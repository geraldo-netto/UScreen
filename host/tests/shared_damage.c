/* T570: retained pixels stay immutable while producer-private histories merge.
 * Compare every published slot against a full conversion, including missed
 * updates, chroma alignment, clipped damage and generation replacement. */
typedef struct {
    frame_exchange_t frames;
    conv_pool_t pool;
    atomic_int running;
    capture_context_t capture;
    raw_ring_t ring;
    int peer;
    unsigned char snapshots[RAW_SLOTS][18 * 10 * 3 / 2];
} t570_fixture;

static void t570_init(t570_fixture *f, int scale) {
    int pair[2]; assert(socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_NONBLOCK, 0, pair) == 0);
    f->ring = (raw_ring_t)RAW_RING_INITIALIZER;
    f->ring.nonce = 57;
    assert(raw_ring_init(&f->ring, pair[0]) && raw_ring_resize(&f->ring, 18, 10));
    f->peer = pair[1];
    f->frames = (frame_exchange_t)FRAME_EXCHANGE_INITIALIZER;
    frame_exchange_init(&f->frames);
    f->frames.width = 18; f->frames.height = 10; f->frames.buffers_ready = 1;
    f->pool = (conv_pool_t)CONV_POOL_INITIALIZER;
    f->capture = (capture_context_t)CAPTURE_INITIALIZER(&f->frames, &f->pool, &f->running);
    f->capture.scale = scale; f->capture.raw_ring = &f->ring;
    f->capture.mode_w = 18 * scale + 1; f->capture.mode_h = 10 * scale + 1;
    f->capture.mode_stride = f->capture.mode_w * 4 + 32;
    f->capture.fb_size = f->capture.mode_stride * f->capture.mode_h;
    f->capture.framebuffer = calloc(1, f->capture.fb_size);
    assert(f->capture.framebuffer);
    f->capture.have_mode = 1;
    mock_monotonic_ms = 1000;
}

static unsigned char *t570_pixels(t570_fixture *f, unsigned slot) {
    return f->ring.memory + RAW_CONTROL_BYTES + slot * f->ring.slot_bytes;
}

static void t570_publish(t570_fixture *f, unsigned slot) {
    unsigned char expected[sizeof(f->snapshots[0])];
    mock_monotonic_ms += 200;
    assert(publish_shared_capture(&f->capture));
    bgra_to_nv12(&f->capture, f->capture.framebuffer, expected, NULL, NULL);
    assert(memcmp(t570_pixels(f, slot), expected, sizeof(expected)) == 0);
    memcpy(f->snapshots[slot], expected, sizeof(expected));
    t418_drain(f->peer);
}

static void t570_release(t570_fixture *f, unsigned slot) {
    atomic_store_explicit((_Atomic uint32_t *)(f->ring.memory + slot * RAW_SLOT_CONTROL_BYTES),
                          RAW_FREE, memory_order_release);
}

static void t570_mutate(t570_fixture *f, unsigned step) {
    int x = step * 7 % (f->capture.mode_w - 1);
    int y = step * 11 % (f->capture.mode_h - 1);
    memset(f->capture.framebuffer + y * f->capture.mode_stride + x * 4, step, 8);
    struct evdi_rect rect = {.x1=x, .x2=x+2, .y1=y, .y2=y+1};
    mark_damage(&f->capture, &rect, 1);
    publish_frame(&f->capture);
}

static void t570_retained(t570_fixture *f) {
    for (unsigned step = 1; step <= 96; step++) {
        t570_mutate(f, step);
        for (unsigned slot = 0; slot < RAW_SLOTS; slot++)
            assert(memcmp(t570_pixels(f, slot), f->snapshots[slot], sizeof(f->snapshots[slot])) == 0);
        assert(publish_shared_capture(&f->capture) && f->capture.raw_pending);
        /* Slot 3 misses 80 updates. Its accumulated history must survive. */
        unsigned slot = step < 80 ? step % 3 : step % RAW_SLOTS;
        t570_release(f, slot);
        t570_publish(f, slot);
        assert(!f->capture.raw_pending);
    }
}

static void t570_bounds(t570_fixture *f) {
    const int ends[] = {INT_MIN, -1, 0, 1, 17, 18, 19, INT_MAX};
    for (unsigned i = 0; i < sizeof(ends) / sizeof(ends[0]); i++) {
        for (unsigned j = 0; j < sizeof(ends) / sizeof(ends[0]); j++) {
            raw_ring_damage(&f->ring, ends[i], ends[j], ends[j], ends[i], f->capture.scale);
            t570_release(f, 0); t570_publish(f, 0);
        }
    }
    raw_ring_damage(&f->ring, 0, 0, 18, 10, 0);
    raw_ring_damage(&f->ring, 0, 0, 18, 10, 5);
    raw_ring_converted(&f->ring, UINT32_MAX);
}

static void t570_generation(t570_fixture *f) {
    unsigned char *old = mmap(NULL, f->ring.bytes, PROT_READ | PROT_WRITE, MAP_SHARED, f->ring.fd, 0);
    assert(old != MAP_FAILED);
    size_t bytes = f->ring.bytes;
    uint64_t generation = f->ring.generation;
    for (unsigned failure = 1; failure <= RAW_SLOTS * 2; failure++) {
        allocation_countdown = failure;
        assert(!raw_ring_resize(&f->ring, 18, 10));
        assert(f->ring.generation == generation);
        assert(memcmp(t570_pixels(f, 0), f->snapshots[0], sizeof(f->snapshots[0])) == 0);
    }
    allocation_countdown = 0;
    assert(raw_ring_resize(&f->ring, 18, 10));
    assert(f->ring.generation == generation + 1);
    atomic_store_explicit((_Atomic uint32_t *)old, RAW_FREE, memory_order_release);
    t570_publish(f, 0);
    assert(memcmp(old + RAW_CONTROL_BYTES, f->snapshots[0], sizeof(f->snapshots[0])) == 0);
    assert(munmap(old, bytes) == 0);
}

static void t570_grab(t570_fixture *f, int rectangles) {
    memset(f->capture.framebuffer, 123, f->capture.fb_size);
    t497_capture_events = 1; t497_rectangles = rectangles;
    grab_now(&f->capture);
    t497_capture_events = 0;
    t570_release(f, 0); t570_publish(f, 0);
}

static void t570_finish(t570_fixture *f) {
    raw_ring_close(&f->ring); close(f->peer);
    free(f->capture.framebuffer);
    pthread_cond_destroy(&f->frames.ready); pthread_mutex_destroy(&f->frames.mutex);
    conv_pool_destroy(&f->pool);
}

static void test_t570_histories(void) {
    for (int scale = 1; scale <= 4; scale++) {
        t570_fixture f = {0}; t570_init(&f, scale);
        for (unsigned slot = 0; slot < RAW_SLOTS; slot++) t570_publish(&f, slot);
        t570_retained(&f);
        t570_bounds(&f);
        t570_generation(&f);
        t570_grab(&f, 64);
        t570_grab(&f, 1); /* Same pixels: exercise the native rectangle adapter. */
        t570_finish(&f);
    }
}
