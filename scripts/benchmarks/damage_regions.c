/* T554: one-session changed-region replay. No EVDI, encoder or device access.
 * Build against pre-T554 modules with -DROW_BASELINE for paired comparisons. */
#define _GNU_SOURCE
#include "conversion.h"
#include "frame_exchange.h"
#include "conversion_exchange.h"
#include <assert.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

enum { WIDTH = 1280, HEIGHT = 800, SAMPLES = 128 };
struct rect { int x0, y0, x1, y1; };
struct replay {
    frame_exchange_t frames;
    conv_pool_t pool;
    frame_cursor_t cursor;
    unsigned char *source;
    int scale;
};

static uint64_t now_ns(clockid_t clock) {
    struct timespec ts;
    assert(clock_gettime(clock, &ts) == 0);
    return (uint64_t)ts.tv_sec * 1000000000 + ts.tv_nsec;
}

static void convert(struct replay *r) {
    frame_exchange_t *f = &r->frames;
    conv_job_t job = {.src = r->source, .ydst = f->fill, .uvdst = f->fill + f->width * f->height,
        .w = WIDTH, .h = HEIGHT, .stride = WIDTH * 4, .ow = f->width, .oh = f->height,
        .scale = r->scale, .cy0 = 0, .cy1 = f->chroma_rows, .dirty = f->dirty_fill};
#ifndef ROW_BASELINE
    job.spans = f->spans_fill;
#endif
    conv_pool_convert(&r->pool, &job);
}

static void rotate(struct replay *r) {
    replay_publish(&r->frames);
    atomic_int running = 1;
    frame_lease_t lease;
    assert(frame_exchange_claim(&r->frames, &r->cursor, &running, &(struct timespec){0}, &lease) == 1);
    frame_exchange_release(&r->frames);
}

static void mark(struct replay *r, struct rect rect) {
#ifdef ROW_BASELINE
    frame_exchange_damage(&r->frames, rect.y0, rect.y1, r->scale);
#else
    frame_exchange_damage_rect(&r->frames, rect.x0, rect.y0, rect.x1, rect.y1, r->scale);
#endif
}

static void mutate(struct replay *r, struct rect rect, int step) {
    for (int y = rect.y0; y < rect.y1; y++)
        memset(r->source + (size_t)y * WIDTH * 4 + rect.x0 * 4, step,
               (size_t)(rect.x1 - rect.x0) * 4);
}

static void initialize(struct replay *r, int scale) {
    r->scale = scale;
    r->source = malloc(WIDTH * HEIGHT * 4);
    assert(r->source);
    for (size_t i = 0; i < WIDTH * HEIGHT * 4; i++) r->source[i] = (unsigned char)(i * 53 + i / 997);
    frame_exchange_init(&r->frames);
    frame_exchange_resize(&r->frames, (WIDTH / scale) & ~1, (HEIGHT / scale) & ~1);
    assert(frame_exchange_allocated(&r->frames));
    r->frames.buffers_ready = 1;
    conv_pool_start(&r->pool, 8);
    for (int i = 0; i < 6; i++) { convert(r); rotate(r); }
}

static int compare(const void *a, const void *b) {
    uint64_t x = *(const uint64_t *)a, y = *(const uint64_t *)b;
    return (x > y) - (x < y);
}

static uint64_t checksum(const frame_exchange_t *f) {
    uint64_t hash = UINT64_C(14695981039346656037);
    for (int i = 0; i < f->size; i++) hash = (hash ^ f->write[i]) * UINT64_C(1099511628211);
    return hash;
}

static void measure(struct replay *r, int mode) {
    const struct rect cases[] = {{100, 100, 164, 164}, {100, 0, 108, HEIGHT},
        {320, 220, 960, 580}, {0, 0, WIDTH, HEIGHT}, {0, 100, 16, 164}};
    struct rect rect = cases[mode], far = {WIDTH - 16, 100, WIDTH, 164};
    uint64_t conversion[SAMPLES], damage[SAMPLES];
    uint64_t start_cpu = now_ns(CLOCK_PROCESS_CPUTIME_ID);
    for (int step = 0; step < SAMPLES; step++) {
        mutate(r, rect, step);
        if (mode == 4) mutate(r, far, step);
        uint64_t start = now_ns(CLOCK_MONOTONIC);
        pthread_mutex_lock(&r->frames.mutex);
        mark(r, rect);
        if (mode == 4) mark(r, far);
        pthread_mutex_unlock(&r->frames.mutex);
        damage[step] = now_ns(CLOCK_MONOTONIC) - start;
        start = now_ns(CLOCK_MONOTONIC);
        convert(r);
        conversion[step] = now_ns(CLOCK_MONOTONIC) - start;
        rotate(r);
    }
    double cpu_us = (now_ns(CLOCK_PROCESS_CPUTIME_ID) - start_cpu) / (1000.0 * SAMPLES);
    qsort(conversion, SAMPLES, sizeof(uint64_t), compare);
    qsort(damage, SAMPLES, sizeof(uint64_t), compare);
    printf("{\"scale\":%d,\"case\":%d,\"samples\":%d,\"conversion_p50_us\":%.3f,"
           "\"conversion_p95_us\":%.3f,\"damage_p50_us\":%.3f,\"cpu_us_per_iteration\":%.3f,"
           "\"last_jobs\":%d,\"checksum\":\"%016" PRIx64 "\"}\n",
           r->scale, mode, SAMPLES, conversion[SAMPLES / 2] / 1000.0,
           conversion[SAMPLES * 95 / 100] / 1000.0, damage[SAMPLES / 2] / 1000.0,
           cpu_us, r->pool.last_jobs, checksum(&r->frames));
}

int main(int argc, char **argv) {
    assert(argc == 3);
    int scale = atoi(argv[1]), mode = atoi(argv[2]);
    assert(scale >= 1 && scale <= 4);
    assert(mode >= 0 && mode < 5);
    struct replay r = {.frames = FRAME_EXCHANGE_INITIALIZER, .pool = CONV_POOL_INITIALIZER};
    initialize(&r, scale);
    measure(&r, mode);
    conv_pool_destroy(&r.pool);
    frame_exchange_free(&r.frames);
    pthread_cond_destroy(&r.frames.ready);
    pthread_mutex_destroy(&r.frames.mutex);
    free(r.source);
    return 0;
}
