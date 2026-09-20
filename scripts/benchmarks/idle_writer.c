/* T492 isolated writer experiment. No EVDI, ADB, display or app lifecycle calls.
 * The runner supplies an unchanged or explicitly patched copy of writer.c. */
#define _GNU_SOURCE
#include "frame_exchange.h"
#include "fifo_writer.h"
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static long long monotonic_us(void) {
    struct timespec now;
    assert(clock_gettime(CLOCK_MONOTONIC, &now) == 0);
    return (long long)now.tv_sec * 1000000 + now.tv_nsec / 1000;
}

static size_t observed_write(fifo_writer_t *fifo, const unsigned char *data,
                             size_t size, unsigned generation) {
    long long start = monotonic_us();
    unsigned marker = data[0];
    size_t remaining = fifo_writer_write(fifo, data, size, generation);
    fprintf(stderr, "{\"event\":\"write\",\"start_us\":%lld,\"end_us\":%lld,"
                    "\"marker\":%u,\"remaining\":%zu}\n",
            start, monotonic_us(), marker, remaining);
    return remaining;
}

#define fifo_writer_write observed_write
#include "candidate_writer.c"
#undef fifo_writer_write

static void sleep_until(long long due_us) {
    struct timespec due = {.tv_sec = due_us / 1000000,
                           .tv_nsec = due_us % 1000000 * 1000};
    int result;
    do { result = clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &due, NULL); }
    while (result == EINTR);
    assert(result == 0);
}

static void publish(frame_exchange_t *frames, unsigned marker) {
    memset(frames->fill, (int)marker, (size_t)frames->width * frames->height);
    memset(frames->fill + frames->width * frames->height, 128,
           (size_t)frames->width * frames->height / 2);
    long long now = monotonic_us();
    fprintf(stderr, "{\"event\":\"publish\",\"start_us\":%lld,\"marker\":%u}\n", now, marker);
    frame_exchange_publish(frames, now);
}

static void mixed_damage(frame_exchange_t *frames, long long start, int fps) {
    /* Damage just before candidate keepalive deadlines, then one second motion. */
    unsigned marker = 33;
    for (int i = 1; i <= 4; i++) {
        sleep_until(start + i * 500000 - 1000);
        publish(frames, marker++);
    }
    for (int i = 0; i < fps; i++) {
        sleep_until(start + 3000000 + i * 1000000LL / fps);
        publish(frames, marker++);
    }
    /* Three idle seconds, then immediate wake before the final input drain. */
    sleep_until(start + 7100000);
    publish(frames, marker);
}

int main(int argc, char **argv) {
    assert(argc == 3);
    int fps = atoi(argv[1]), mixed = atoi(argv[2]);
    assert((fps == 30 || fps == 60) && (mixed == 0 || mixed == 1));
    frame_exchange_t frames = FRAME_EXCHANGE_INITIALIZER;
    frame_exchange_init(&frames);
    frame_exchange_resize(&frames, 1280, 800);
    assert(frame_exchange_allocated(&frames));
    frames.buffers_ready = 1;
    atomic_int running = 1;
    fifo_writer_t fifo = FIFO_WRITER_INITIALIZER(&running, &frames.generation);
    fifo.fd = dup(STDOUT_FILENO);
    assert(fifo.fd >= 0);
    assert(fcntl(fifo.fd, F_SETFL, O_NONBLOCK) == 0);
    writer_context_t writer = {&frames, &fifo, &running, fps};
    long long start = monotonic_us();
    publish(&frames, 32);
    pthread_t thread;
    assert(pthread_create(&thread, NULL, writer_run, &writer) == 0);
    if (mixed) mixed_damage(&frames, start, fps);
    sleep_until(start + (mixed ? 7500000 : 6000000));
    pthread_mutex_lock(&frames.mutex);
    /* End between complete frames. Deliberate mid-frame shutdown is covered
     * by the production quarantine regressions, not this idle timing trial. */
    while (atomic_load(&frames.writer_busy))
        pthread_cond_wait(&frames.ready, &frames.mutex);
    atomic_store(&running, 0);
    pthread_cond_broadcast(&frames.ready);
    pthread_mutex_unlock(&frames.mutex);
    assert(pthread_join(thread, NULL) == 0);
    fifo_writer_close(&fifo);
    frame_exchange_free(&frames);
    return 0;
}
