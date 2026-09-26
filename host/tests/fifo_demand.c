/* T580: retained EVDI ownership must not convert frames nobody consumes. */
static void t580_stop_writer(pthread_t writer) {
    pthread_mutex_lock(&g_frames.mutex);
    g_running = 0;
    pthread_cond_broadcast(&g_frames.ready);
    pthread_mutex_unlock(&g_frames.mutex);
    assert(pthread_join(writer, NULL) == 0);
    fifo_writer_close(&g_fifo);
    t497_free_capture();
}

static void test_t580_no_reader(void) {
    alarm(10);
    char directory[] = "/tmp/blent-t580-XXXXXX", path[256];
    assert(mkdtemp(directory));
    snprintf(path, sizeof(path), "%s/frames", directory);
    assert(mkfifo(path, 0600) == 0);
    frame_exchange_init(&g_frames);
    pthread_t writer;
    assert(start_capture_writer(path, &writer));
    g_conversion.last_jobs = 0; /* Reset the pool's diagnostic initializer. */
    struct evdi_mode mode = {8, 8, 60, 32, 0x34325258};
    on_mode_changed(mode, &g_capture);
    assert(g_capture.buffer_registered && g_capture.have_mode);
    assert(g_conversion.last_jobs == 0 &&
           "T580: no FIFO reader must mean no NV12 conversion jobs");
    t497_capture_events = 1;
    t497_rectangles = 1;
    g_capture.update_pending = 1;
    on_update_ready(0, &g_capture);
    assert(!g_capture.update_pending && g_capture.grab_count == 1);
    assert(g_conversion.last_jobs == 0 && !g_frames.latest_valid);
    t580_stop_writer(writer);
    assert(unlink(path) == 0 && rmdir(directory) == 0);
}

typedef struct {
    char directory[64], path[128];
    pthread_t writer;
} t580_fifo_t;

static void t580_start(t580_fifo_t *fixture) {
    alarm(10);
    signal(SIGPIPE, SIG_IGN);
    strcpy(fixture->directory, "/tmp/blent-t580-fifo-XXXXXX");
    assert(mkdtemp(fixture->directory));
    snprintf(fixture->path, sizeof(fixture->path), "%s/frames", fixture->directory);
    assert(mkfifo(fixture->path, 0600) == 0);
    frame_exchange_init(&g_frames);
    assert(start_capture_writer(fixture->path, &fixture->writer));
}

static void t580_finish(t580_fifo_t *fixture) {
    t580_stop_writer(fixture->writer);
    assert(unlink(fixture->path) == 0 && rmdir(fixture->directory) == 0);
}

static int t580_connected(void) {
    pthread_mutex_lock(&g_frames.mutex);
    int connected = g_frames.reader_connected;
    pthread_mutex_unlock(&g_frames.mutex);
    return connected;
}

static void t580_wait_reader(int expected) {
    long long deadline = now_ms() + 2000;
    while (t580_connected() != expected && now_ms() < deadline) usleep(1000);
    assert(t580_connected() == expected);
}

static void t580_poll_capture(void) {
    struct evdi_event_context context = {.user_data = &g_capture};
    struct pollfd fds[2] = {{.fd = -1}, {.fd = capture_wakeup_fd(&g_capture), .events = POLLIN}};
    long long start = now_ms();
    assert(poll_capture_events(EVDI_INVALID_HANDLE, &context, fds, 2000) == 1);
    assert(now_ms() - start < 1000 && "T580: reconnect wakes capture without EVDI damage");
}

static void t580_check_frame(int reader) {
    unsigned char expected[512], actual[512];
    size_t size = (size_t)g_frames.size;
    assert(size <= sizeof(expected));
    bgra_to_nv12(&g_capture, g_capture.framebuffer, expected, NULL, NULL);
    size_t received = 0;
    while (received < size) {
        struct pollfd fd = {.fd = reader, .events = POLLIN};
        assert(poll(&fd, 1, 2000) == 1);
        ssize_t count = read(reader, actual + received, size - received);
        assert(count > 0);
        received += (size_t)count;
    }
    assert(memcmp(actual, expected, size) == 0 && "T580: reconnect delivers a full fresh NV12 frame");
}

static int t580_resume(t580_fifo_t *fixture) {
    int reader = open(fixture->path, O_RDONLY | O_NONBLOCK);
    assert(reader >= 0);
    t580_wait_reader(1);
    t580_poll_capture();
    t580_check_frame(reader);
    return reader;
}

static void t580_paused_pixels(unsigned char value) {
    assert(!t580_connected());
    memset(g_capture.framebuffer, value, g_capture.fb_size);
    /* Missing damage history must never leak an old/partially converted frame
     * to a new reader. Resume must invalidate all three cached histories. */
    memset(g_frames.dirty_fill, 0, g_frames.dirty_bytes);
    memset(g_frames.dirty_latest, 0, g_frames.dirty_bytes);
    memset(g_frames.dirty_write, 0, g_frames.dirty_bytes);
    g_conversion.last_jobs = 0;
    publish_frame(&g_capture);
    assert(g_conversion.last_jobs == 0);
}

static void test_t580_reconnect(void) {
    t580_fifo_t fixture;
    t580_start(&fixture);
    struct evdi_mode mode = {8, 8, 60, 32, 0x34325258};
    on_mode_changed(mode, &g_capture);
    t580_paused_pixels(0x65);
    int reader = t580_resume(&fixture);
    close(reader);
    t580_wait_reader(0);
    t580_paused_pixels(0x9a);
    reader = t580_resume(&fixture);
    close(reader);
    t580_wait_reader(0);
    mode.width = 12; mode.height = 10;
    on_mode_changed(mode, &g_capture);
    t580_paused_pixels(0x37);
    reader = t580_resume(&fixture);
    assert(g_frames.size == 180 && g_capture.grab_count == 0);
    close(reader);
    t580_finish(&fixture);
}

static void test_t580_generation(void) {
    alarm(10);
    frame_exchange_t frames = FRAME_EXCHANGE_INITIALIZER;
    frame_exchange_init(&frames);
    assert(frame_exchange_enable_demand(&frames));
    assert(frame_exchange_enable_demand(&frames)); /* Idempotent startup retry. */
    assert(fcntl(frames.demand_fd, F_GETFD) & FD_CLOEXEC);
    assert(fcntl(frames.demand_fd, F_GETFL) & O_NONBLOCK);
    frame_exchange_resize(&frames, 8, 8);
    frames.buffers_ready = 1;
    unsigned generation;
    assert(!frame_exchange_begin(&frames, &generation));
    frame_exchange_reader(&frames, 1);
    assert(frame_exchange_take_request(&frames));
    assert(frame_exchange_begin(&frames, &generation));
    frame_exchange_reader(&frames, 0);
    frame_exchange_reader(&frames, 1);
    frame_exchange_publish(&frames, 1, generation);
    assert(!frames.latest_valid && frames.refresh_needed && frames.dirty_fill[0]);
    assert(frame_exchange_begin(&frames, &generation));
    memset(frames.fill, 0x71, frames.size);
    frame_exchange_publish(&frames, 2, generation);
    assert(!frame_exchange_take_request(&frames));
    frame_cursor_t cursor = {.generation = UINT_MAX};
    frame_lease_t lease;
    atomic_int running = 1;
    assert(frame_exchange_claim(&frames, &cursor, &running, NULL, &lease) == 1);
    assert(lease.fresh && lease.data[0] == 0x71);
    frame_exchange_release(&frames);
    frame_exchange_reader(&frames, 0);
    struct timespec expired = {0};
    assert(frame_exchange_claim(&frames, &cursor, &running, &expired, &lease) == 0);
    assert(!cursor.have_frame && !frame_exchange_take_request(&frames));
    assert(frame_exchange_retire(&frames));
    frame_exchange_resize(&frames, 12, 10);
    frames.buffers_ready = 1;
    frame_exchange_reader(&frames, 1);
    assert(frame_exchange_begin(&frames, &generation));
    memset(frames.fill, 0x29, frames.size);
    frame_exchange_publish(&frames, 3, generation);
    assert(frame_exchange_claim(&frames, &cursor, &running, NULL, &lease) == 1);
    assert(lease.fresh && lease.size == 180 && lease.data[179] == 0x29);
    frame_exchange_release(&frames);
    frame_exchange_free(&frames);
    pthread_cond_destroy(&frames.ready);
    pthread_mutex_destroy(&frames.mutex);
}

static void test_t580_unavailable(void) {
    t580_fifo_t fixture;
    t580_eventfd_failure = 1;
    t580_start(&fixture);
    struct evdi_mode mode = {8, 8, 60, 32, 0x34325258};
    g_conversion.last_jobs = 0;
    on_mode_changed(mode, &g_capture);
    assert(g_frames.demand_fd == -1 && g_conversion.last_jobs > 0);
    int reader = open(fixture.path, O_RDONLY | O_NONBLOCK);
    assert(reader >= 0);
    t580_check_frame(reader); /* Legacy publication needs no new capture event. */
    close(reader);
    t580_finish(&fixture);
}

static void test_t580_early_reader(void) {
    t580_fifo_t fixture;
    t580_start(&fixture);
    g_conversion.last_jobs = 0;
    int reader = open(fixture.path, O_RDONLY | O_NONBLOCK);
    assert(reader >= 0);
    t580_wait_reader(1);
    t580_poll_capture();
    assert(g_conversion.last_jobs == 0 && !g_frames.latest_valid);
    assert(frame_exchange_take_request(&g_frames)); /* No mode cannot consume demand. */
    struct evdi_mode mode = {8, 8, 60, 32, 0x34325258};
    on_mode_changed(mode, &g_capture);
    t580_check_frame(reader);
    close(reader);
    t580_finish(&fixture);
}

static void t580_fresh_lease(frame_exchange_t *frames, unsigned generation, unsigned char value) {
    memset(frames->fill, value, frames->size);
    frame_exchange_publish(frames, 1, generation);
    frame_cursor_t cursor = {.generation = UINT_MAX};
    frame_lease_t lease;
    atomic_int running = 1;
    assert(frame_exchange_claim(frames, &cursor, &running, NULL, &lease) == 1);
    assert(lease.size == (size_t)frames->size && lease.fresh);
    memset(frames->fill, value ^ 0xff, frames->size);
    frame_exchange_publish(frames, 2, generation);
    for (size_t index = 0; index < lease.size; index++)
        assert(lease.data[index] == value && "T580: fresh publication must preserve a writer lease");
    frame_exchange_release(frames);
}

static void t580_mutate_transition(frame_exchange_t *frames, unsigned seed) {
    frame_exchange_reader(frames, 1);
    assert(frame_exchange_retire(frames));
    /* Resize with a connected reader, including minimum 2x2 NV12. */
    frame_exchange_resize(frames, 2 * (1 + seed % 16), 2 * (1 + (seed >> 8) % 16));
    frames->buffers_ready = 1;
    unsigned generation;
    assert(frame_exchange_begin(frames, &generation));
    frame_exchange_reader(frames, 0);
    frame_exchange_reader(frames, seed & 1);
    frame_exchange_publish(frames, 0, generation);
    assert(!frames->latest_valid && frames->dirty_fill[0]);
    frame_exchange_reader(frames, 1);
    assert(frame_exchange_begin(frames, &generation));
    t580_fresh_lease(frames, generation, (unsigned char)seed);
}

static void test_t580_transitions(void) {
    alarm(10);
    frame_exchange_t frames = FRAME_EXCHANGE_INITIALIZER;
    frame_exchange_init(&frames);
    assert(frame_exchange_enable_demand(&frames));
    frames.generation = UINT_MAX - 2; /* Bounded sequence fuzzing crosses wrap. */
    unsigned seed = 580;
    for (int iteration = 0; iteration < 512; iteration++) {
        seed = seed * 1664525u + 1013904223u;
        t580_mutate_transition(&frames, seed);
    }
    assert(!frame_exchange_take_request(&frames)); /* Coalesced hints are not state. */
    frame_exchange_free(&frames);
    pthread_cond_destroy(&frames.ready);
    pthread_mutex_destroy(&frames.mutex);
}
