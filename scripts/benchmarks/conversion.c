/* T383: isolated conversion-pool/triple-buffer replay; never opens EVDI. */
#define _GNU_SOURCE
#include "conversion.h"
#include "frame_exchange.h"
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <time.h>

#define WIDTH 1920
#define HEIGHT 1080
#define MAX_SESSIONS 4
#define MAX_SAMPLES 256

static uint64_t clock_ns(clockid_t clock) {
    struct timespec ts;
    assert(clock_gettime(clock, &ts) == 0);
    return (uint64_t)ts.tv_sec * 1000000000 + ts.tv_nsec;
}

struct lane {
    conv_pool_t pool;
    frame_exchange_t frames;
    frame_cursor_t cursor;
    unsigned char *source;
    int scale, damage, samples, stride, width, height;
    uint64_t conversion[MAX_SAMPLES], marking[MAX_SAMPLES], locking[MAX_SAMPLES];
    uint64_t logical_bytes;
    uint64_t checksum;
    pthread_barrier_t *start, *end;
};

static void mark_work(struct lane *lane) {
    frame_exchange_t *f = &lane->frames;
    if (lane->damage == 0) return;
    if (lane->damage == 3) { frame_exchange_mark_all(f); return; }
    if (lane->damage == 1) {
        for (int cy = 0; cy < f->chroma_rows; cy += 64)
            frame_exchange_damage(f, cy * 2 * lane->scale, (cy + 1) * 2 * lane->scale, lane->scale);
        return;
    }
    /* Below capture's 64-rectangle overflow threshold; heavy overlap. */
    for (int r = 0; r < 63; r++)
        frame_exchange_damage(f, lane->height / 4 + r % 16, lane->height * 3 / 4 + r % 16, lane->scale);
}

static void convert(struct lane *lane) {
    frame_exchange_t *f = &lane->frames;
    conv_job_t frame = {lane->source, f->fill, f->fill + f->width * f->height,
        lane->width, lane->height, lane->stride, f->width, f->height, lane->scale,
        0, f->chroma_rows, f->dirty_fill, NULL};
    conv_pool_convert(&lane->pool, &frame);
}

static void rotate(struct lane *lane) {
    frame_exchange_publish(&lane->frames, 0);
    atomic_int running = 1;
    frame_lease_t lease;
    assert(frame_exchange_claim(&lane->frames, &lane->cursor, &running, &(struct timespec){0}, &lease) == 1);
    frame_exchange_release(&lane->frames);
}

static uint64_t checksum(const unsigned char *data, size_t size) {
    uint64_t hash = UINT64_C(14695981039346656037);
    for (size_t i = 0; i < size; i++) hash = (hash ^ data[i]) * UINT64_C(1099511628211);
    return hash;
}

static void initialize(struct lane *lane, int id, int workers) {
    lane->stride = lane->width * 4 + 64;
    lane->source = malloc((size_t)lane->stride * lane->height);
    assert(lane->source);
    for (size_t i = 0; i < (size_t)lane->stride * lane->height; i++)
        lane->source[i] = (unsigned char)((i * 37 + i / 997 + id * 71) % 256);
    frame_exchange_init(&lane->frames);
    frame_exchange_resize(&lane->frames, lane->width / lane->scale, lane->height / lane->scale);
    assert(frame_exchange_allocated(&lane->frames));
    lane->frames.buffers_ready = 1;
    conv_pool_start(&lane->pool, workers);
    /* Populate every buffer/history before timed empty/sparse frames. */
    for (int i = 0; i < 6; i++) { convert(lane); rotate(lane); }
    mark_work(lane);
    for (int cy = 0; cy < lane->frames.chroma_rows; cy++) {
        if (lane->frames.dirty_fill[cy / 8] & (1u << (cy % 8)))
            lane->logical_bytes += (uint64_t)lane->frames.width * (8 * lane->scale * lane->scale + 3);
    }
}

static void *run_lane(void *arg) {
    struct lane *lane = arg;
    pthread_barrier_wait(lane->start);
    for (int n = 0; n < lane->samples; n++) {
        uint64_t start = clock_ns(CLOCK_MONOTONIC);
        pthread_mutex_lock(&lane->frames.mutex);
        uint64_t acquired = clock_ns(CLOCK_MONOTONIC);
        lane->locking[n] = acquired - start;
        mark_work(lane);
        lane->marking[n] = clock_ns(CLOCK_MONOTONIC) - acquired;
        pthread_mutex_unlock(&lane->frames.mutex);
        start = clock_ns(CLOCK_MONOTONIC);
        convert(lane);
        lane->conversion[n] = clock_ns(CLOCK_MONOTONIC) - start;
        rotate(lane);
    }
    pthread_barrier_wait(lane->end);
    pthread_barrier_wait(lane->start); /* exclude checksum CPU from the timed phase */
    lane->checksum = checksum(lane->frames.write, lane->frames.size);
    return NULL;
}

static void destroy(struct lane *lane) {
    conv_pool_destroy(&lane->pool);
    frame_exchange_free(&lane->frames);
    pthread_cond_destroy(&lane->frames.ready);
    pthread_mutex_destroy(&lane->frames.mutex);
    free(lane->source);
}

static int compare(const void *a, const void *b) {
    uint64_t x = *(const uint64_t *)a, y = *(const uint64_t *)b;
    return (x > y) - (x < y);
}

static int last_jobs(const conv_pool_t *pool) {
#ifdef USCREEN_ADAPTIVE_POOL
    return pool->last_jobs;
#else
    return pool->count;
#endif
}

static void print_lanes(struct lane *lanes, int sessions) {
    for (int i = 0; i < sessions; i++) {
        int n = lanes[i].samples;
        qsort(lanes[i].conversion, (size_t)n, sizeof(uint64_t), compare);
        qsort(lanes[i].marking, (size_t)n, sizeof(uint64_t), compare);
        qsort(lanes[i].locking, (size_t)n, sizeof(uint64_t), compare);
        printf("%s{\"convert_p50_ns\":%llu,\"convert_p99_ns\":%llu,\"damage_p50_ns\":%llu,\"checksum\":\"%016llx\",\"pool_capacity\":%d,\"last_jobs\":%d,\"lock_wait_p99_ns\":%llu,\"logical_bytes\":%llu}",
            i ? "," : "", (unsigned long long)lanes[i].conversion[(n - 1) / 2],
            (unsigned long long)lanes[i].conversion[n - 1 - n / 100],
            (unsigned long long)lanes[i].marking[(n - 1) / 2],
            (unsigned long long)lanes[i].checksum, lanes[i].pool.count, last_jobs(&lanes[i].pool),
            (unsigned long long)lanes[i].locking[n - 1 - n / 100],
            (unsigned long long)lanes[i].logical_bytes);
    }
}

static long resident_peak_kib(void) {
    FILE *status = fopen("/proc/self/status", "r");
    assert(status);
    char line[256];
    long peak = -1;
    while (fgets(line, sizeof(line), status)) {
        if (sscanf(line, "VmHWM: %ld", &peak) == 1) break;
    }
    fclose(status);
    return peak;
}

static void report(struct lane *lanes, int sessions, const struct rusage *before,
                   const struct rusage *after, uint64_t wall_ns, uint64_t cpu_ns) {
    printf("{\"wall_ns\":%llu,\"cpu_us\":%llu,\"voluntary_switches\":%ld,\"involuntary_switches\":%ld,\"rss_peak_kib\":%ld,\"lanes\":[",
        (unsigned long long)wall_ns, (unsigned long long)(cpu_ns / 1000),
        after->ru_nvcsw - before->ru_nvcsw, after->ru_nivcsw - before->ru_nivcsw, resident_peak_kib());
    print_lanes(lanes, sessions);
    puts("]}");
}

static int arguments(int argc, char **argv, int *sessions, int *workers,
                     int *scale, int *damage, int *samples) {
    if (argc < 6) return 0;
    *sessions = atoi(argv[1]); *workers = atoi(argv[2]); *scale = atoi(argv[3]);
    *damage = atoi(argv[4]); *samples = atoi(argv[5]);
    return *sessions >= 1 && *sessions <= MAX_SESSIONS && *workers >= 1
        && *workers <= 128 && *scale >= 1 && *scale <= 4 && *damage >= 0 && *damage <= 3;
}

int main(int argc, char **argv) {
    assert(argc == 6 || argc == 7);
    int multiplier = argc == 7 ? atoi(argv[6]) : 1;
    assert(multiplier >= 1 && multiplier <= 4);
    int sessions, workers, scale, damage, samples;
    assert(arguments(argc, argv, &sessions, &workers, &scale, &damage, &samples));
    assert(samples >= 1 && samples <= MAX_SAMPLES);
    struct lane lanes[MAX_SESSIONS];
    pthread_t threads[MAX_SESSIONS];
    pthread_barrier_t start, end;
    assert(pthread_barrier_init(&start, NULL, sessions + 1) == 0);
    assert(pthread_barrier_init(&end, NULL, sessions + 1) == 0);
    for (int i = 0; i < sessions; i++) {
        lanes[i] = (struct lane){.pool = CONV_POOL_INITIALIZER, .frames = FRAME_EXCHANGE_INITIALIZER,
            .scale = scale, .damage = damage, .samples = samples, .width = WIDTH * multiplier, .height = HEIGHT * multiplier, .start = &start, .end = &end};
        initialize(&lanes[i], i, workers);
        assert(pthread_create(&threads[i], NULL, run_lane, &lanes[i]) == 0);
    }
    struct rusage before, after;
    getrusage(RUSAGE_SELF, &before);
    uint64_t cpu_start = clock_ns(CLOCK_PROCESS_CPUTIME_ID);
    uint64_t start_ns = clock_ns(CLOCK_MONOTONIC);
    pthread_barrier_wait(&start);
    pthread_barrier_wait(&end);
    uint64_t wall_ns = clock_ns(CLOCK_MONOTONIC) - start_ns;
    uint64_t cpu_ns = clock_ns(CLOCK_PROCESS_CPUTIME_ID) - cpu_start;
    getrusage(RUSAGE_SELF, &after);
    pthread_barrier_wait(&start);
    for (int i = 0; i < sessions; i++) pthread_join(threads[i], NULL);
    report(lanes, sessions, &before, &after, wall_ns, cpu_ns);
    for (int i = 0; i < sessions; i++) destroy(&lanes[i]);
    pthread_barrier_destroy(&start); pthread_barrier_destroy(&end);
    return 0;
}
