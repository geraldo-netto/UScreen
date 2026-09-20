/* T554: bounded damage fuzz, independent full-frame oracle and buffer leases. */
static pixel_span_t expected_axis(int first, int last, int scale, int blocks) {
    pixel_span_t span = {0};
    if (first > last) { int swap = first; first = last; last = swap; }
    if (first == last) return span;
    for (int block = 0; block < blocks; block++) {
        if ((int64_t)first >= (int64_t)(block + 1) * 2 * scale) continue;
        if ((int64_t)last <= (int64_t)block * 2 * scale) continue;
        if (span.end == 0) span.begin = block;
        span.end = block + 1;
    }
    return span;
}

static void check_region_history(const frame_exchange_t *f, const unsigned char *mask,
                                 const pixel_span_t *spans, pixel_span_t x, pixel_span_t y) {
    for (int cy = 0; cy < f->chroma_rows; cy++) {
        int dirty = x.begin < x.end && cy >= y.begin && cy < y.end;
        assert(!!(mask[cy / 8] & (1u << (cy % 8))) == dirty && "T554: dirty rows differ from block intersections");
        if (!dirty) continue;
        assert(spans[cy].begin == x.begin * 2 && spans[cy].end == x.end * 2
            && "T554: damage spans differ from block intersections");
    }
}

static void check_region_bounds(frame_exchange_t *f, const int *rect, int scale) {
    memset(f->dirty_fill, 0, f->dirty_bytes);
    memset(f->dirty_latest, 0, f->dirty_bytes);
    memset(f->dirty_write, 0, f->dirty_bytes);
    frame_exchange_damage_rect(f, rect[0], rect[1], rect[2], rect[3], scale);
    pixel_span_t x = expected_axis(rect[0], rect[2], scale, f->width / 2);
    pixel_span_t y = expected_axis(rect[1], rect[3], scale, f->chroma_rows);
    check_region_history(f, f->dirty_fill, f->spans_fill, x, y);
    check_region_history(f, f->dirty_latest, f->spans_latest, x, y);
    check_region_history(f, f->dirty_write, f->spans_write, x, y);
}

static void region_bounds(void) {
    const int cases[][4] = {
        {INT_MIN, INT_MIN, INT_MAX, INT_MAX}, {INT_MAX, INT_MAX, INT_MIN, INT_MIN},
        {INT_MIN, 0, -1, INT_MAX}, {INT_MAX - 1, 0, INT_MAX, INT_MAX},
        {0, INT_MIN, INT_MAX, -1}, {0, INT_MAX - 1, INT_MAX, INT_MAX},
        {5, 7, 5, 9}, {5, 7, 9, 7}, {0, 0, 1, 1}, {125, 97, 999, 999},
        {-9, -8, 9, 11}, {22, 32, 3, 5},
    };
    frame_exchange_t f = FRAME_EXCHANGE_INITIALIZER;
    frame_exchange_resize(&f, 126, 98);
    assert(frame_exchange_allocated(&f));
    for (int scale = 1; scale <= 4; scale++) {
        for (size_t n = 0; n < sizeof(cases) / sizeof(cases[0]); n++) check_region_bounds(&f, cases[n], scale);
        uint32_t state = (uint32_t)scale;
        for (int n = 0; n < 512; n++) {
            int rect[4];
            for (int i = 0; i < 4; i++) {
                state = state * 1664525u + 1013904223u;
                rect[i] = (int)(state % 1600) - 400;
            }
            check_region_bounds(&f, rect, scale);
        }
    }
    frame_exchange_free(&f);
    pthread_mutex_destroy(&f.mutex);
}

static void invalid_region_scale(void) {
    frame_exchange_t f = FRAME_EXCHANGE_INITIALIZER;
    frame_exchange_damage_rect(&f, 0, 0, 8, 8, 1);
    frame_exchange_damage(&f, 0, 8, 1);
    frame_exchange_resize(&f, 8, 8);
    const int scales[] = {INT_MIN, -1, 0, 5, INT_MAX};
    for (size_t i = 0; i < sizeof(scales) / sizeof(scales[0]); i++) {
        memset(f.dirty_fill, 0, f.dirty_bytes);
        frame_exchange_damage_rect(&f, 0, 0, 8, 8, scales[i]);
        frame_exchange_damage(&f, 0, 8, scales[i]);
        assert(f.dirty_fill[0] == 0 && "T554: invalid scale must not divide or mark damage");
    }
    frame_exchange_free(&f);
    pthread_mutex_destroy(&f.mutex);
}

static void mutate_region(unsigned char *source, int stride, const int *r, int value) {
    for (int y = r[1]; y < r[3]; y++)
        for (int x = r[0]; x < r[2]; x++)
            for (int channel = 0; channel < 4; channel++)
                source[y * stride + x * 4 + channel] = (unsigned char)(value * 31 + x + y * 7 + channel * 53);
}

static void convert_region_frame(conv_pool_t *pool, frame_exchange_t *f, conv_job_t job,
                                 unsigned char *expected) {
    job.ydst = f->fill; job.uvdst = f->fill + f->width * f->height;
    job.dirty = f->dirty_fill; job.spans = f->spans_fill;
    conv_pool_convert(pool, &job);
    job.ydst = expected; job.uvdst = expected + f->width * f->height;
    job.dirty = NULL; job.spans = NULL;
    reference(&job);
    assert(memcmp(f->fill, expected, f->size) == 0 && "T554: missed damage in recycled buffer history");
}

static void region_lifecycle(int scale) {
    enum { W = 62, H = 46, SIZE = W * H * 3 / 2 };
    int sw = W * scale + 1, sh = H * scale + 1, stride = sw * 4 + 32;
    unsigned char *source = calloc(stride, sh), expected[SIZE], held[SIZE];
    assert(source);
    frame_exchange_t f = FRAME_EXCHANGE_INITIALIZER;
    frame_exchange_init(&f);
    frame_exchange_resize(&f, W, H);
    f.buffers_ready = 1;
    conv_pool_t pool = CONV_POOL_INITIALIZER;
    conv_pool_start(&pool, 4);
    conv_job_t job = {source, NULL, NULL, sw, sh, stride, W, H, scale, 0, H / 2, NULL, NULL};
    frame_cursor_t cursor = {0};
    frame_lease_t lease = {0};
    atomic_int running = 1;
    for (int step = 0; step < 40; step++) {
        int r[4] = {step % 7, step % 9, step % 7 + 3, step % 9 + 5};
        mutate_region(source, stride, r, step);
        frame_exchange_damage_rect(&f, r[0], r[1], r[2], r[3], scale);
        int far[4] = {sw - 5, r[1], sw - 1, r[3]};
        mutate_region(source, stride, far, step);
        frame_exchange_damage_rect(&f, far[0], far[1], far[2], far[3], scale);
        if (step == 35) frame_exchange_mark_all(&f);
        convert_region_frame(&pool, &f, job, expected);
        frame_exchange_publish(&f, step);
        if (lease.data) assert(memcmp(lease.data, held, SIZE) == 0 && "T554: conversion overwrote held writer pixels");
        if (step % 3 == 0) continue; /* Drop latest frames while writer holds an older lease. */
        frame_exchange_release(&f);
        assert(frame_exchange_claim(&f, &cursor, &running, &(struct timespec){0}, &lease) == 1);
        assert(memcmp(lease.data, expected, SIZE) == 0 && "T554: dropped frames lost damage");
        memcpy(held, lease.data, SIZE);
        if (step != 20) continue;
        frame_exchange_release(&f);
        assert(frame_exchange_retire(&f));
        frame_exchange_resize(&f, W, H);
        f.buffers_ready = 1;
        lease.data = NULL;
    }
    frame_exchange_release(&f);
    frame_exchange_free(&f);
    pthread_cond_destroy(&f.ready);
    pthread_mutex_destroy(&f.mutex);
    conv_pool_destroy(&pool);
    free(source);
}

static void region_dispatch(void) {
    enum { W = 4096, H = 256, SIZE = W * H * 3 / 2 };
    conv_pool_t pool = CONV_POOL_INITIALIZER;
    conv_pool_start(&pool, 8);
    unsigned char *source = calloc(W * H, 4), *actual = malloc(SIZE), *expected = malloc(SIZE);
    assert(source);
    assert(actual && expected);
    unsigned char dirty[H / 16];
    memset(dirty, 0xFF, sizeof(dirty));
    pixel_span_t spans[H / 2];
    conv_job_t job = {source, actual, actual + W * H, W, H, W * 4, W, H, 1, 0, H / 2, dirty, spans};
    for (int width = 2; width <= 2048; width *= 1024) {
        memset(actual, 0xA5, SIZE); memset(expected, 0xA5, SIZE);
        for (int cy = 0; cy < H / 2; cy++) spans[cy] = (pixel_span_t){100, 100 + width};
        conv_pool_convert(&pool, &job);
        assert(pool.last_jobs == (width == 2 ? 1 : 2) && "T554: dispatch must account for horizontal work");
        conv_job_t oracle = job;
        oracle.ydst = expected; oracle.uvdst = expected + W * H;
        for (int cy = 0; cy < H / 2; cy++)
            for (int x = 100; x < 100 + width; x += 2) reference_block(&oracle, x, cy);
        assert(memcmp(actual, expected, SIZE) == 0 && "T554: threaded spans changed pixels outside damage");
    }
    conv_pool_destroy(&pool);
    free(source); free(actual); free(expected);
}

static void region_scaled_workers(conv_pool_t *pool, int scale) {
    enum { SW = 127, SH = 99, STRIDE = SW * 4 + 32, GUARD = 32 };
    int ow = (SW / scale) & ~1, oh = (SH / scale) & ~1, size = ow * oh * 3 / 2;
    unsigned char source[STRIDE * SH], actual[SW * SH * 3 / 2 + GUARD], expected[sizeof(actual)];
    pixel_span_t spans[SH / 2];
    for (size_t i = 0; i < sizeof(source); i++) source[i] = (unsigned char)(i * 53 + i / 997);
    memset(actual, 0xA5, sizeof(actual)); memset(expected, 0xA5, sizeof(expected));
    for (int cy = 0; cy < oh / 2; cy++) {
        int begin = 2 * (cy * 7 % (ow / 2));
        spans[cy] = (pixel_span_t){begin, begin + 2 * (cy % 2)};
    }
    conv_job_t job = {source, actual, actual + ow * oh, SW, SH, STRIDE, ow, oh,
        scale, 0, oh / 2, NULL, spans};
    conv_pool_convert(pool, &job);
    assert(pool->last_jobs == pool->count);
    job.ydst = expected; job.uvdst = expected + ow * oh;
    for (int cy = 0; cy < oh / 2; cy++)
        for (int x = spans[cy].begin; x < spans[cy].end; x += 2) reference_block(&job, x, cy);
    assert(memcmp(actual, expected, size + GUARD) == 0 && "T554: scaled threaded span conversion differs from oracle");
}

static void region_kernel_matrix(void) {
    for (int workers = 1; workers <= 8; workers *= 8) {
        conv_pool_t pool = CONV_POOL_INITIALIZER;
        conv_pool_start(&pool, workers);
        for (int scale = 1; scale <= 4; scale++) region_scaled_workers(&pool, scale);
        conv_pool_destroy(&pool);
    }
}

static void region_suite(void) {
    region_bounds();
    invalid_region_scale();
    for (int scale = 1; scale <= 4; scale++) region_lifecycle(scale);
    region_dispatch();
    region_kernel_matrix();
}
