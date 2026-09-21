/* T418: production conversion, control validation and capture pacing. */
static void t418_control(raw_ring_t *ring, int peer, uint32_t kind) {
    raw_ring_t sender = *ring;
    sender.socket = peer;
    assert(raw_send(&sender, kind, 0, 0) == 1);
}

static void t418_drain(int socket) {
    unsigned char bytes[RAW_MESSAGE_BYTES];
    struct iovec vector = {bytes, sizeof(bytes)};
    union { struct cmsghdr aligned; char bytes[CMSG_SPACE(8 * sizeof(int))]; } control;
    for (;;) {
        struct msghdr message = {.msg_iov = &vector, .msg_iovlen = 1,
            .msg_control = control.bytes, .msg_controllen = sizeof(control)};
        ssize_t count = recvmsg(socket, &message, MSG_DONTWAIT | MSG_CMSG_CLOEXEC);
        if (count < 0) break;
        raw_close_rights(&message);
        assert(count == RAW_MESSAGE_BYTES);
    }
}

static void test_t418_ring(void) {
    raw_ring_t ring = RAW_RING_INITIALIZER;
    int pair[2]; assert(socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_NONBLOCK, 0, pair) == 0);
    assert(!raw_ring_init(&ring, -1));
    int unconnected = socket(AF_UNIX, SOCK_SEQPACKET, 0);
    assert(unconnected >= 0 && !raw_ring_init(&ring, unconnected));
    close(unconnected);
    assert(raw_ring_init(&ring, pair[0]));
    assert(raw_ring_resize(&ring, 64, 64));
    uint32_t slot;
    assert(!raw_ring_acquire(&ring, &slot)); /* No consumer handshake yet. */
    raw_ring_t hello = ring; hello.nonce = 7;
    t418_control(&hello, pair[1], 4);
    assert(raw_ring_service(&ring) && ring.memory);
    t418_drain(pair[1]);
    for (unsigned i = 0; i < RAW_SLOTS; i++) {
        assert(raw_ring_acquire(&ring, &slot));
        assert(slot == i);
        assert(raw_ring_publish(&ring, slot, i * 100) == 1);
    }
    assert(!raw_ring_acquire(&ring, &slot));
    assert(raw_ring_publish(&ring, 0, 0) == -1);
    assert(raw_ring_publish(&ring, UINT32_MAX, 0) == -1);
    t418_control(&ring, pair[1], 3);
    assert(raw_ring_service(&ring));
    assert(!raw_ring_acquire(&ring, &slot)); /* Message alone cannot release a slot. */
    atomic_store_explicit((_Atomic uint32_t *)ring.memory, RAW_FREE, memory_order_release);
    assert(raw_ring_acquire(&ring, &slot));
    assert(raw_ring_publish(&ring, slot, 0) == 1);
    t418_drain(pair[1]);
    for (uint32_t dimension = 0; dimension < 100; dimension++) {
        int valid = dimension >= 2 && !(dimension & 1);
        assert(raw_ring_resize(&ring, dimension, 2) == valid);
        t418_drain(pair[1]);
    }
    assert(!raw_ring_resize(&ring, UINT32_MAX, 2));
    unsigned char bytes[RAW_MESSAGE_BYTES];
    raw_message(&ring, bytes, 4, 0, 0);
    for (int index = 0; index < RAW_MESSAGE_BYTES; index++) {
        unsigned char old = bytes[index]; bytes[index] ^= 0x80;
        /* Decode fuzz is bounded; any accepted hello only allocates validated geometry. */
        raw_control(&ring, bytes);
        bytes[index] = old;
        t418_drain(pair[1]);
    }
    /* Ancillary fd on a control message is rejected and closed. */
    t418_control(&ring, pair[1], 1);
    assert(!raw_ring_service(&ring));
    close(pair[1]);
    assert(!raw_ring_service(&ring));
    assert(raw_ring_resize(&ring, 64, 64) == 0);
    raw_ring_close(&ring);
    raw_ring_close(&ring);
}

static void test_t418_capture(void) {
    raw_ring_t ring = RAW_RING_INITIALIZER;
    int pair[2]; assert(socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_NONBLOCK, 0, pair) == 0);
    assert(raw_ring_init(&ring, pair[0]));
    frame_exchange_init(&g_frames);
    g_capture.raw_ring = &ring;
    g_running = 1;
    assert(service_shared_capture(&g_capture)); /* Partial startup with no mode. */
    on_mode_changed((struct evdi_mode){.width=64, .height=64, .refresh_rate=60,
        .bits_per_pixel=32, .pixel_format=0x34325258}, &g_capture);
    assert(g_capture.have_mode);
    assert(g_capture.raw_pending);
    assert(!g_frames.fill);
    raw_ring_t hello = ring; hello.nonce = 8;
    t418_control(&hello, pair[1], 4);
    mock_monotonic_ms = 1000;
    assert(service_shared_capture(&g_capture));
    assert(!g_capture.raw_pending && ring.sequence == 1);
    assert(shared_poll_timeout(&g_capture, 250) == 200);
    assert(publish_shared_capture(&g_capture) && ring.sequence == 1);
    memset(g_capture.framebuffer, 0xff, g_capture.fb_size);
    mark_all_dirty(&g_capture);
    g_capture.raw_pending = 1;
    assert(shared_poll_timeout(&g_capture, 250) == 17);
    mock_monotonic_ms += 17;
    assert(publish_shared_capture(&g_capture));
    assert(ring.sequence == 2);
    unsigned char *pixels = ring.memory + RAW_CONTROL_BYTES + ring.slot_bytes;
    assert(pixels[0] == 235 && pixels[64 * 64] == 128);
    for (int i = 0; i < 2; i++) {
        mock_monotonic_ms += 200; assert(publish_shared_capture(&g_capture));
    }
    memset(g_capture.framebuffer, 0, g_capture.fb_size);
    mark_all_dirty(&g_capture);
    g_capture.raw_pending = 1;
    mock_monotonic_ms += 200;
    assert(publish_shared_capture(&g_capture) && g_capture.raw_pending);
    assert(shared_poll_timeout(&g_capture, 250) == 1);
    atomic_store_explicit((_Atomic uint32_t *)ring.memory, RAW_FREE, memory_order_release);
    assert(publish_shared_capture(&g_capture) && !g_capture.raw_pending);
    assert(ring.memory[RAW_CONTROL_BYTES] == 16); /* Final fresh update survives a full ring. */
    t418_drain(pair[1]);
    close(pair[1]);
    assert(!service_shared_capture(&g_capture));
    on_mode_changed((struct evdi_mode){.width=66, .height=64, .refresh_rate=60,
        .bits_per_pixel=32, .pixel_format=0x34325258}, &g_capture);
    assert(g_capture.capture_failed && !g_running);
    raw_ring_close(&ring);
    free(g_capture.framebuffer); g_capture.framebuffer = NULL;
    frame_exchange_free(&g_frames);
    g_capture.raw_ring = NULL;
}

static void test_t418_startup(void) {
    assert(configure_raw_socket(NULL));
    const char *invalid[] = {"", "-1", "999999999999999999999999", "3x", "2147483648"};
    for (unsigned i = 0; i < sizeof(invalid)/sizeof(invalid[0]); i++) assert(!configure_raw_socket(invalid[i]));
    int pair[2]; assert(socketpair(AF_UNIX, SOCK_SEQPACKET, 0, pair) == 0);
    char text[30]; snprintf(text, sizeof(text), "%d", pair[0]);
    assert(configure_raw_socket(text));
    pthread_t writer = 0;
    g_conversion_threads = 1;
    assert(start_capture_writer(NULL, &writer) && writer == 0);
    conv_pool_destroy(&g_conversion);
    raw_ring_close(&g_raw_ring); close(pair[1]);
}

/* T570: repeating an already converted slot must do no conversion work. */
static void test_t570_idle(void) {
    frame_exchange_t frames = FRAME_EXCHANGE_INITIALIZER;
    conv_pool_t pool = CONV_POOL_INITIALIZER;
    atomic_int running = 1;
    capture_context_t capture = CAPTURE_INITIALIZER(&frames, &pool, &running);
    raw_ring_t ring = RAW_RING_INITIALIZER;
    int pair[2]; assert(socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_NONBLOCK, 0, pair) == 0);
    assert(raw_ring_init(&ring, pair[0]));
    frame_exchange_init(&frames);
    capture.raw_ring = &ring;
    on_mode_changed((struct evdi_mode){.width=64, .height=64, .refresh_rate=60,
        .bits_per_pixel=32, .pixel_format=0x34325258}, &capture);
    raw_ring_t hello = ring; hello.nonce = 57;
    t418_control(&hello, pair[1], 4);
    mock_monotonic_ms = 1000;
    assert(service_shared_capture(&capture));
    t418_drain(pair[1]);
    atomic_store_explicit((_Atomic uint32_t *)ring.memory, RAW_FREE, memory_order_release);
    mock_monotonic_ms += 200;
    assert(publish_shared_capture(&capture) && ring.sequence == 2);
    assert(pool.last_jobs == 0 && "T570: idle repeat must reuse immutable converted pixels");
    raw_ring_close(&ring); close(pair[1]);
    free(capture.framebuffer);
    frame_exchange_free(&frames);
    pthread_cond_destroy(&frames.ready); pthread_mutex_destroy(&frames.mutex);
    conv_pool_destroy(&pool);
}
