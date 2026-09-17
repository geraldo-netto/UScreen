/* T405/T389: isolated raw-pipe capacity experiment; no EVDI, ADB or FFmpeg.
 * Each session has its own producer/reader, as in the capture pipeline. */
#define _GNU_SOURCE
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/resource.h>
#include <time.h>
#include <unistd.h>

#define MAX_SESSIONS 4
#define MAX_FRAMES 4096
struct session {
    int reader, writer, capacity, frames, paced, delay_ms;
    size_t size;
    unsigned char *payload;
    pthread_t producer, consumer;
    pthread_barrier_t *start;
    uint64_t reads, writes, polls, pending;
    uint64_t write_ns[MAX_FRAMES], age_ns[MAX_FRAMES];
};

static uint64_t now(clockid_t clock) {
    struct timespec ts;
    assert(clock_gettime(clock, &ts) == 0);
    return (uint64_t)ts.tv_sec * 1000000000 + ts.tv_nsec;
}

static void pace(uint64_t deadline) {
    struct timespec ts = {.tv_sec = (time_t)(deadline / 1000000000),
        .tv_nsec = (long)(deadline % 1000000000)};
    int result;
    do { result = clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &ts, NULL); }
    while (result == EINTR);
    assert(result == 0);
}

static void start(pthread_barrier_t *barrier) {
    int result = pthread_barrier_wait(barrier);
    assert(result == 0 || result == PTHREAD_BARRIER_SERIAL_THREAD);
}

static void read_exact(struct session *session, unsigned char *data, size_t size) {
    size_t offset = 0;
    while (offset < size) {
        ssize_t count = read(session->reader, data + offset, size - offset);
        session->reads++;
        if (count < 0 && errno == EINTR) continue;
        assert(count > 0);
        offset += (size_t)count;
    }
}

static uint64_t receive_frame(struct session *session, int frame) {
    uint64_t header[2];
    unsigned char buffer[65536];
    read_exact(session, (unsigned char *)header, sizeof(header));
    assert(header[0] == (uint64_t)frame);
    for (size_t offset = sizeof(header); offset < session->size;) {
        size_t size = session->size - offset;
        if (size > sizeof(buffer)) size = sizeof(buffer);
        read_exact(session, buffer, size);
        assert(memcmp(buffer, session->payload + offset, size) == 0);
        offset += size;
    }
    return now(CLOCK_MONOTONIC) - header[1];
}

static void *consume(void *argument) {
    struct session *session = argument;
    start(session->start);
    for (int frame = 0; frame < session->frames; frame++) {
        session->age_ns[frame] = receive_frame(session, frame);
        if (session->delay_ms)
            pace(now(CLOCK_MONOTONIC) + (uint64_t)session->delay_ms * 1000000);
    }
    return NULL;
}

static void wait_writable(struct session *session) {
    struct pollfd fd = {.fd = session->writer, .events = POLLOUT};
    int result;
    do { session->polls++; result = poll(&fd, 1, -1); } while (result < 0 && errno == EINTR);
    assert(result > 0 && (fd.revents & POLLOUT));
}

static void sample_pending(struct session *session) {
    int pending;
    assert(ioctl(session->writer, FIONREAD, &pending) == 0);
    if ((uint64_t)pending > session->pending) session->pending = (uint64_t)pending;
}

static void send_frame(struct session *session) {
    size_t offset = 0;
    while (offset < session->size) {
        ssize_t count = write(session->writer, session->payload + offset, session->size - offset);
        session->writes++;
        if (count < 0 && errno == EINTR) continue;
        if (count < 0 && errno == EAGAIN) { wait_writable(session); continue; }
        assert(count > 0);
        offset += (size_t)count;
        sample_pending(session);
    }
}

static void *produce(void *argument) {
    struct session *session = argument;
    start(session->start);
    uint64_t previous = 0;
    for (int frame = 0; frame < session->frames; frame++) {
        /* No catch-up bursts: capture is latest-only before FIFO admission. */
        if (session->paced) pace(previous + 1000000000 / 60);
        uint64_t header[2] = {(uint64_t)frame, now(CLOCK_MONOTONIC)};
        previous = header[1];
        memcpy(session->payload, header, sizeof(header));
        send_frame(session);
        session->write_ns[frame] = now(CLOCK_MONOTONIC) - header[1];
    }
    return NULL;
}

static void initialize(struct session *session, pthread_barrier_t *barrier, char **argv) {
    int pipe[2];
    assert(pipe2(pipe, O_CLOEXEC) == 0);
    session->reader = pipe[0]; session->writer = pipe[1]; session->start = barrier;
    int requested = atoi(argv[1]) * 1024 * 1024;
    session->capacity = fcntl(pipe[1], F_SETPIPE_SZ, requested);
    /* Do not silently label a capped fallback as a successful larger-pipe trial. */
    if (session->capacity < 0) { perror("F_SETPIPE_SZ"); exit(77); }
    assert(session->capacity == fcntl(pipe[1], F_GETPIPE_SZ));
    assert(fcntl(pipe[1], F_SETFL, O_NONBLOCK) == 0);
    session->size = (size_t)strtoul(argv[3], NULL, 10);
    session->frames = atoi(argv[4]); session->paced = atoi(argv[5]); session->delay_ms = atoi(argv[6]);
    session->payload = malloc(session->size);
    assert(session->payload);
    for (size_t i = 0; i < session->size; i++) session->payload[i] = (unsigned char)(i * 37 + i / 997);
    assert(pthread_create(&session->consumer, NULL, consume, session) == 0);
    assert(pthread_create(&session->producer, NULL, produce, session) == 0);
}

static int compare(const void *a, const void *b) {
    uint64_t x = *(const uint64_t *)a, y = *(const uint64_t *)b;
    return (x > y) - (x < y);
}

static void report_session(struct session *session, int index) {
    int frames = session->frames;
    qsort(session->write_ns, (size_t)frames, sizeof(uint64_t), compare);
    qsort(session->age_ns, (size_t)frames, sizeof(uint64_t), compare);
    int p50 = (frames - 1) / 2, p99 = frames - 1 - frames / 100;
    if (index) printf(",");
    printf("{\"capacity\":%d,\"reads\":%llu,\"writes\":%llu,\"polls\":%llu,\"pending_max_bytes\":%llu,"
        "\"write_p50_ns\":%llu,\"write_p99_ns\":%llu,\"age_p50_ns\":%llu,\"age_p99_ns\":%llu}",
        session->capacity, (unsigned long long)session->reads,
        (unsigned long long)session->writes, (unsigned long long)session->polls,
        (unsigned long long)session->pending,
        (unsigned long long)session->write_ns[p50], (unsigned long long)session->write_ns[p99],
        (unsigned long long)session->age_ns[p50], (unsigned long long)session->age_ns[p99]);
}

static void measure(struct session *sessions, int count, pthread_barrier_t *barrier) {
    struct rusage before, after;
    assert(getrusage(RUSAGE_SELF, &before) == 0);
    uint64_t cpu = now(CLOCK_PROCESS_CPUTIME_ID), wall = now(CLOCK_MONOTONIC);
    start(barrier);
    for (int i = 0; i < count; i++) {
        assert(pthread_join(sessions[i].producer, NULL) == 0);
        assert(pthread_join(sessions[i].consumer, NULL) == 0);
    }
    wall = now(CLOCK_MONOTONIC) - wall; cpu = now(CLOCK_PROCESS_CPUTIME_ID) - cpu;
    assert(getrusage(RUSAGE_SELF, &after) == 0);
    printf("{\"cpu_ns\":%llu,\"wall_ns\":%llu,\"voluntary_switches\":%ld,\"involuntary_switches\":%ld,"
        "\"minor_faults\":%ld,\"major_faults\":%ld,\"max_rss_kib\":%ld,\"channels\":[",
        (unsigned long long)cpu, (unsigned long long)wall,
        after.ru_nvcsw - before.ru_nvcsw, after.ru_nivcsw - before.ru_nivcsw,
        after.ru_minflt - before.ru_minflt, after.ru_majflt - before.ru_majflt, after.ru_maxrss);
    for (int i = 0; i < count; i++) report_session(&sessions[i], i);
    puts("]}");
}

static void validate(char **argv) {
    int capacity = atoi(argv[1]), count = atoi(argv[2]), frames = atoi(argv[4]);
    size_t size = (size_t)strtoul(argv[3], NULL, 10);
    assert(capacity >= 1 && capacity <= 32);
    assert(count >= 1 && count <= MAX_SESSIONS);
    assert(size >= 16 && size <= 32 * 1024 * 1024);
    assert(frames >= 1 && frames <= MAX_FRAMES);
    assert(atoi(argv[6]) >= 0 && atoi(argv[6]) <= 100);
}

int main(int argc, char **argv) {
    assert(argc == 7);
    validate(argv);
    int count = atoi(argv[2]);
    pthread_barrier_t barrier;
    assert(pthread_barrier_init(&barrier, NULL, (unsigned)count * 2 + 1) == 0);
    struct session *sessions = calloc((size_t)count, sizeof(*sessions));
    assert(sessions);
    for (int i = 0; i < count; i++) initialize(&sessions[i], &barrier, argv);
    measure(sessions, count, &barrier);
    for (int i = 0; i < count; i++) {
        close(sessions[i].reader); close(sessions[i].writer); free(sessions[i].payload);
    }
    free(sessions);
    assert(pthread_barrier_destroy(&barrier) == 0);
    return 0;
}
