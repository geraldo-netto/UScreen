/* T554: exercise the real EVDI rectangle adapter and conversion path without
 * attaching a display. Sentinel pixels expose writes outside reported damage. */
static void t554_assert_plane(const unsigned char *plane, int width, int height,
                              int first_y, int end_y, int value) {
    for (int y = 0; y < height; y++) {
        for (int x = 0; x < width; x++) {
            int inside = x >= 4 && x < 8 && y >= first_y && y < end_y;
            assert(plane[y * width + x] == (inside ? value : 0xA5)
                && "T554: conversion overwrote pixels outside horizontal damage");
        }
    }
}

static void t554_capture_rectangle(int scale) {
    enum { W = 16, H = 8 };
    frame_exchange_t frames = FRAME_EXCHANGE_INITIALIZER;
    conv_pool_t pool = CONV_POOL_INITIALIZER;
    atomic_int running = 1;
    capture_context_t capture = CAPTURE_INITIALIZER(&frames, &pool, &running);
    frame_exchange_init(&frames);
    frame_exchange_resize(&frames, W, H);
    assert(frame_exchange_allocated(&frames));
    frames.buffers_ready = 1;
    capture.scale = scale;
    capture.mode_w = W * scale + 1;
    capture.mode_h = H * scale + 1;
    capture.mode_stride = capture.mode_w * 4 + 32;
    capture.framebuffer = calloc(capture.mode_stride, capture.mode_h);
    assert(capture.framebuffer);
    /* Populate/clear every history before submitting one narrow rectangle. */
    frame_cursor_t cursor = {0};
    frame_lease_t lease;
    for (int i = 0; i < 3; i++) {
        publish_frame(&capture);
        assert(frame_exchange_claim(&frames, &cursor, &running, &(struct timespec){0}, &lease) == 1);
        frame_exchange_release(&frames);
    }
    memset(frames.fill, 0xA5, frames.size);
    struct evdi_rect rect = {.x1 = 5 * scale, .x2 = 7 * scale, .y1 = 3 * scale, .y2 = 5 * scale};
    mark_damage(&capture, &rect, 1);
    publish_frame(&capture);
    t554_assert_plane(frames.latest, W, H, 2, 6, 16);
    t554_assert_plane(frames.latest + W * H, W, H / 2, 1, 3, 128);
    free(capture.framebuffer);
    frame_exchange_free(&frames);
    pthread_cond_destroy(&frames.ready);
    pthread_mutex_destroy(&frames.mutex);
    conv_pool_destroy(&pool);
}

static void test_t554_regions(void) {
    for (int scale = 1; scale <= 4; scale++) t554_capture_rectangle(scale);
}
