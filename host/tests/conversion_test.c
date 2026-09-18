/* T383: independent scalar oracle, dirty histories and dispatch contracts. */
#include "conversion.h"
#include "frame_exchange.h"
#include <assert.h>
#include <stdint.h>
#include <limits.h>
#include <stdlib.h>
#include <string.h>

struct rgb { int r, g, b; };

static struct rgb mean_pixel(const conv_job_t *j, int x, int y) {
    struct rgb sum = {0};
    for (int dy = 0; dy < j->scale; dy++) {
        for (int dx = 0; dx < j->scale; dx++) {
            const unsigned char *p = j->src + (size_t)(y * j->scale + dy) * j->stride + (x * j->scale + dx) * 4;
            sum.b += p[0]; sum.g += p[1]; sum.r += p[2];
        }
    }
    int count = j->scale * j->scale;
    sum.b /= count; sum.g /= count; sum.r /= count;
    return sum;
}

static int floor_256(int value) {
    return value >= 0 ? value / 256 : -((-value + 255) / 256);
}

static void reference_block(const conv_job_t *j, int x, int cy) {
    struct rgb sum = {0};
    for (int q = 0; q < 4; q++) {
        int px = x + q % 2, py = 2 * cy + q / 2;
        struct rgb p = mean_pixel(j, px, py);
        j->ydst[py * j->ow + px] = (unsigned char)((47 * p.r + 157 * p.g + 16 * p.b + 128) / 256 + 16);
        sum.b += p.b; sum.g += p.g; sum.r += p.r;
    }
    int b = sum.b / 4, g = sum.g / 4, r = sum.r / 4;
    j->uvdst[cy * j->ow + x] = (unsigned char)(floor_256(-26 * r - 86 * g + 112 * b + 128) + 128);
    j->uvdst[cy * j->ow + x + 1] = (unsigned char)(floor_256(112 * r - 102 * g - 10 * b + 128) + 128);
}

static void reference(const conv_job_t *j) {
    for (int cy = j->cy0; cy < j->cy1; cy++) {
        if (j->dirty && !(j->dirty[cy / 8] & (1u << (cy % 8)))) continue;
        for (int x = 0; x < j->ow; x += 2) reference_block(j, x, cy);
    }
}

static void dirty_mask(unsigned char *mask, int rows, int pattern) {
    memset(mask, 0, (size_t)(rows + 7) / 8);
    for (int row = 0; row < rows; row++) {
        if (pattern >= 3 || (pattern == 1 && row == 3) || (pattern == 2 && row % 5 == 0))
            mask[row / 8] |= (unsigned char)(1u << (row % 8));
    }
}

static void check_conversion(conv_pool_t *pool, int scale, int pattern) {
    enum { W = 127, H = 99, STRIDE = 544, GUARD = 32 };
    unsigned char *source = malloc(STRIDE * H);
    int ow = (W / scale) & ~1, oh = (H / scale) & ~1;
    size_t size = (size_t)ow * oh * 3 / 2;
    unsigned char *actual = malloc(size + GUARD), *expected = malloc(size + GUARD);
    assert(source && actual && expected);
    for (size_t i = 0; i < STRIDE * H; i++) source[i] = (unsigned char)(i * 53 + i / 997);
    memset(actual, 0xA5, size + GUARD); memset(expected, 0xA5, size + GUARD);
    unsigned char mask[64];
    dirty_mask(mask, oh / 2, pattern);
    conv_job_t frame = {source, actual, actual + ow * oh, W, H, STRIDE, ow, oh, scale, 0, oh / 2,
        pattern == 4 ? NULL : mask};
    conv_pool_convert(pool, &frame);
    frame.ydst = expected; frame.uvdst = expected + ow * oh;
    reference(&frame);
    assert(memcmp(actual, expected, size + GUARD) == 0 && "T383: conversion/rounding/dirty/guard mismatch");
    free(source); free(actual); free(expected);
}

static void conversion_matrix(void) {
    for (int workers = 1; workers <= 8; workers *= 2) {
        conv_pool_t pool = CONV_POOL_INITIALIZER;
        conv_pool_start(&pool, workers);
        for (int scale = 1; scale <= 4; scale++)
            for (int pattern = 0; pattern < 5; pattern++) check_conversion(&pool, scale, pattern);
        conv_pool_destroy(&pool);
    }
}

static void expected_damage(unsigned char *mask, int y0, int y1, int scale) {
    if (y1 < y0) { int swap = y0; y0 = y1; y1 = swap; }
    int first = y0 / (2 * scale), end = (y1 + 2 * scale - 1) / (2 * scale);
    for (int cy = 0; cy < 65; cy++) {
        if (cy >= first && cy < end) mask[cy / 8] |= (unsigned char)(1u << (cy % 8));
    }
}

static void check_damage(int y0, int y1, int scale) {
    unsigned char masks[3][11], expected[3][11];
    for (int history = 0; history < 3; history++) memset(masks[history], 0x11 * (history + 1), 11);
    memcpy(expected, masks, sizeof(masks));
    frame_exchange_t frames = FRAME_EXCHANGE_INITIALIZER;
    frames.dirty_fill = masks[0] + 1; frames.dirty_latest = masks[1] + 1; frames.dirty_write = masks[2] + 1;
    frames.chroma_rows = 65; frames.dirty_bytes = 9;
    frame_exchange_damage(&frames, y0, y1, scale);
    for (int history = 0; history < 3; history++) expected_damage(expected[history] + 1, y0, y1, scale);
    assert(memcmp(masks, expected, sizeof(masks)) == 0 && "T383: dirty histories or guard bytes changed");
    pthread_mutex_destroy(&frames.mutex);
}

static void damage_matrix(void) {
    const int ranges[][2] = {{0, 0}, {-9, 3}, {1, 7}, {13, 17}, {8, 64}, {129, 900}, {900, 129}, {0, 900}, {-9, -3}};
    for (int scale = 1; scale <= 4; scale++)
        for (size_t range = 0; range < sizeof(ranges) / sizeof(ranges[0]); range++)
            check_damage(ranges[range][0], ranges[range][1], scale);
}

static void extreme_damage_case(int first, int end, int scale, int all) {
    unsigned char masks[3][11], expected[11] = {0};
    expected[0] = expected[10] = 0xA5;
    for (int i = 0; i < 3; i++) memcpy(masks[i], expected, sizeof(expected));
    if (all) { memset(expected + 1, 0xFF, 8); expected[9] = 1; }
    frame_exchange_t frames = FRAME_EXCHANGE_INITIALIZER;
    frames.dirty_fill = masks[0] + 1; frames.dirty_latest = masks[1] + 1; frames.dirty_write = masks[2] + 1;
    frames.chroma_rows = 65; frames.dirty_bytes = 9;
    frame_exchange_damage(&frames, first, end, scale);
    for (int i = 0; i < 3; i++)
        assert(memcmp(masks[i], expected, sizeof(expected)) == 0 && "T452: extreme damage lost rows or changed canaries");
    pthread_mutex_destroy(&frames.mutex);
}

static void extreme_damage(void) {
    const int ranges[][3] = {
        {INT_MIN, INT_MAX, 1}, {INT_MAX, INT_MIN, 1}, {0, INT_MAX, 1},
        {INT_MAX - 1, INT_MAX, 0}, {INT_MIN, -1, 0},
    };
    for (int scale = 1; scale <= 4; scale++)
        for (size_t n = 0; n < sizeof(ranges) / sizeof(ranges[0]); n++)
            extreme_damage_case(ranges[n][0], ranges[n][1], scale, ranges[n][2]);
}

static void sparse_dispatch(void) {
    conv_pool_t pool = CONV_POOL_INITIALIZER;
    conv_pool_start(&pool, 8);
    unsigned char *source = calloc(512 * 256, 4), *output = malloc(512 * 256 * 3 / 2);
    assert(source && output);
    unsigned char dirty[16] = {0};
    conv_job_t job = {source, output, output + 512 * 256, 512, 256, 512 * 4,
        512, 256, 1, 0, 128, dirty};
    unsigned initial = pool.generation;
    conv_pool_convert(&pool, &job);
    assert(pool.generation == initial && "T383: empty work woke every worker");
    dirty[0] = 1;
    conv_pool_convert(&pool, &job);
    assert(pool.generation == initial && "T383: tiny work woke every worker");
    conv_pool_destroy(&pool);
    free(source); free(output);
}

static void large_pool(void) {
    conv_pool_t pool = CONV_POOL_INITIALIZER;
    conv_pool_start(&pool, 128);
    assert(pool.count == 128 && "T383: requested 128-worker pool was truncated");
    for (int scale = 1; scale <= 4; scale++) check_conversion(&pool, scale, 4);
    conv_pool_destroy(&pool);
}

static void density_dispatch(void) {
    enum { W = 4096, H = 256, SIZE = W * H * 3 / 2 };
    conv_pool_t pool = CONV_POOL_INITIALIZER;
    conv_pool_start(&pool, 128);
    unsigned char *source = malloc(W * H * 4), *actual = malloc(SIZE), *expected = malloc(SIZE);
    assert(source && actual && expected);
    memset(source, 173, W * H * 4);
    unsigned char dirty[H / 16];
    memset(dirty, 0xFF, sizeof(dirty));
    conv_job_t job = {source, actual, actual + W * H, W, H, W * 4, W, H, 1, 0, H / 2, dirty};
    conv_job_t oracle = job;
    oracle.ydst = expected; oracle.uvdst = expected + W * H;
    conv_pool_convert(&pool, &job);
    assert(pool.last_jobs == 4 && "T383: large pool must dispatch only sufficient work");
    reference(&oracle);
    assert(memcmp(actual, expected, SIZE) == 0);
    memset(source, 0, W * H * 4);
    memset(dirty, 0, sizeof(dirty));
    for (int row = 32; row < 72; row++) dirty[row / 8] |= (unsigned char)(1u << (row % 8));
    conv_pool_convert(&pool, &job);
    assert(pool.last_jobs == 2 && "T383: clustered damage must shrink the active pool");
    assert(pool.jobs[0].cy1 == 52 && pool.jobs[1].cy0 == 52 && "T383: divide dirty work equally");
    reference(&oracle);
    assert(memcmp(actual, expected, SIZE) == 0 && "T383: clustered dispatch corrupts untouched rows");
    conv_pool_destroy(&pool);
    free(source); free(actual); free(expected);
}

int main(int argc, char **argv) {
    assert(argc == 2);
    if (strcmp(argv[1], "damage-extreme") == 0) extreme_damage();
    else if (strcmp(argv[1], "density") == 0) density_dispatch();
    else if (strcmp(argv[1], "dispatch") == 0) sparse_dispatch();
    else if (strcmp(argv[1], "large") == 0) large_pool();
    else { assert(strcmp(argv[1], "equivalence") == 0); conversion_matrix(); damage_matrix(); }
    return 0;
}
