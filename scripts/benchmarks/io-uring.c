/* T405: isolated FIFO-like pipe/TCP replay; never opens EVDI or ADB.
 * Optional benchmark dependency: liburing. Production does not depend on it. */
#define _GNU_SOURCE
#include <liburing.h>
#include <arpa/inet.h>
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <time.h>
#include <unistd.h>

#define MAX_CHANNELS 4
#define MAX_FRAMES 4096
struct channel {
    int reader, writer, tcp, frames;
    size_t size, offset;
    unsigned char *payload;
    pthread_t thread;
    uint64_t reads, writes;
};

static uint64_t now(clockid_t clock) {
    struct timespec ts;
    assert(clock_gettime(clock, &ts) == 0);
    return (uint64_t)ts.tv_sec * 1000000000 + ts.tv_nsec;
}

static void tcp_pair(int pair[2]) {
    int listener = socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0);
    assert(listener >= 0);
    struct sockaddr_in addr = {.sin_family = AF_INET, .sin_addr.s_addr = htonl(INADDR_LOOPBACK)};
    assert(bind(listener, (struct sockaddr *)&addr, sizeof(addr)) == 0);
    socklen_t length = sizeof(addr);
    assert(getsockname(listener, (struct sockaddr *)&addr, &length) == 0);
    assert(listen(listener, 1) == 0);
    pair[1] = socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0);
    assert(pair[1] >= 0);
    assert(connect(pair[1], (struct sockaddr *)&addr, sizeof(addr)) == 0);
    pair[0] = accept4(listener, NULL, NULL, SOCK_CLOEXEC);
    assert(pair[0] >= 0);
    close(listener);
}

static void read_exact(struct channel *channel, unsigned char *data, size_t size) {
    size_t offset = 0;
    while (offset < size) {
        ssize_t count = read(channel->reader, data + offset, size - offset);
        channel->reads++;
        if (count < 0 && errno == EINTR) continue;
        assert(count > 0);
        offset += (size_t)count;
    }
}

static void *receive(void *arg) {
    struct channel *channel = arg;
    unsigned char buffer[65536];
    for (int frame = 0; frame < channel->frames; frame++) {
        uint64_t sequence;
        read_exact(channel, (unsigned char *)&sequence, sizeof(sequence));
        assert(sequence == (uint64_t)frame);
        for (size_t offset = 8; offset < channel->size;) {
            size_t size = channel->size - offset;
            if (size > sizeof(buffer)) size = sizeof(buffer);
            read_exact(channel, buffer, size);
            assert(memcmp(buffer, channel->payload + offset, size) == 0);
            offset += size;
        }
    }
    return NULL;
}

static void initialize(struct channel *channel, int tcp, size_t size, int frames) {
    int pair[2];
    if (tcp) tcp_pair(pair); else assert(pipe2(pair, O_CLOEXEC) == 0);
    if (!tcp) (void)fcntl(pair[1], F_SETPIPE_SZ, 1 << 20); /* production helper request, best effort */
    assert(fcntl(pair[1], F_SETFL, O_NONBLOCK) == 0);
    *channel = (struct channel){.reader = pair[0], .writer = pair[1], .tcp = tcp,
        .size = size, .frames = frames, .payload = malloc(size)};
    assert(channel->payload);
    for (size_t i = 0; i < size; i++) channel->payload[i] = (unsigned char)(i * 37 + i / 997);
    assert(pthread_create(&channel->thread, NULL, receive, channel) == 0);
}

static void advance(struct channel *channel, int result) {
    if (result == -EINTR || result == -EAGAIN) return;
    assert(result > 0);
    channel->offset += (size_t)result;
    assert(channel->offset <= channel->size);
}

/* Optimistic nonblocking writes, then readiness only if a writer is blocked. */
static void poll_frame(struct channel *channels, int count) {
    for (;;) {
        struct pollfd pending[MAX_CHANNELS];
        int used = 0;
        for (int i = 0; i < count; i++) {
            struct channel *channel = &channels[i];
            if (channel->offset == channel->size) continue;
            ssize_t result = write(channel->writer, channel->payload + channel->offset,
                                   channel->size - channel->offset);
            channel->writes++;
            advance(channel, result < 0 ? -errno : (int)result);
            if (channel->offset < channel->size)
                pending[used++] = (struct pollfd){.fd = channel->writer, .events = POLLOUT};
        }
        if (!used) return;
        int result = poll(pending, (nfds_t)used, -1);
        assert(result > 0 || errno == EINTR);
    }
}

static void queue_write(struct io_uring *ring, struct channel *channel, int id) {
    struct io_uring_sqe *entry = io_uring_get_sqe(ring);
    assert(entry);
    unsigned char *data = channel->payload + channel->offset;
    size_t size = channel->size - channel->offset;
    if (channel->tcp) io_uring_prep_send(entry, channel->writer, data, size, MSG_NOSIGNAL);
    else io_uring_prep_write(entry, channel->writer, data, (unsigned)size, 0);
    io_uring_sqe_set_data64(entry, (uint64_t)id);
    channel->writes++;
}

static void retry_ready(const struct channel *channel, int result) {
    if (result != -EAGAIN) return;
    struct pollfd fd = {.fd = channel->writer, .events = POLLOUT};
    while (poll(&fd, 1, -1) < 0) assert(errno == EINTR);
}

static int complete(struct io_uring *ring, struct channel *channels) {
    int finished = 0;
    struct io_uring_cqe *entry;
    while (io_uring_peek_cqe(ring, &entry) == 0) {
        int id = (int)io_uring_cqe_get_data64(entry), result = entry->res;
        io_uring_cqe_seen(ring, entry);
        struct channel *channel = &channels[id];
        advance(channel, result);
        if (channel->offset == channel->size) finished++;
        else { retry_ready(channel, result); queue_write(ring, channel, id); }
    }
    return finished;
}

static void uring_frame(struct io_uring *ring, struct channel *channels, int count) {
    for (int i = 0; i < count; i++) queue_write(ring, &channels[i], i);
    int remaining = count;
    while (remaining) {
        int result = io_uring_submit_and_wait(ring, 1);
        assert(result >= 0 || result == -EINTR);
        remaining -= complete(ring, channels);
    }
}

static void pace(uint64_t deadline) {
    struct timespec ts = {.tv_sec = (time_t)(deadline / 1000000000),
        .tv_nsec = (long)(deadline % 1000000000)};
    int result;
    do { result = clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &ts, NULL); } while (result == EINTR);
    assert(result == 0);
}

static int compare(const void *a, const void *b) {
    uint64_t x = *(const uint64_t *)a, y = *(const uint64_t *)b;
    return (x > y) - (x < y);
}

static void destroy(struct channel *channels, int count) {
    for (int i = 0; i < count; i++) {
        close(channels[i].reader); close(channels[i].writer); free(channels[i].payload);
    }
}

static void run(struct channel *channels, int count, struct io_uring *ring, int paced) {
    uint64_t durations[MAX_FRAMES], reads = 0, writes = 0;
    int frames = channels[0].frames;
    uint64_t cpu = now(CLOCK_PROCESS_CPUTIME_ID), start = now(CLOCK_MONOTONIC);
    for (int frame = 0; frame < frames; frame++) {
        if (paced) pace(start + (uint64_t)frame * 1000000000 / 60);
        for (int i = 0; i < count; i++) {
            channels[i].offset = 0;
            uint64_t sequence = (uint64_t)frame;
            memcpy(channels[i].payload, &sequence, 8);
        }
        uint64_t before = now(CLOCK_MONOTONIC);
        if (ring) uring_frame(ring, channels, count); else poll_frame(channels, count);
        durations[frame] = now(CLOCK_MONOTONIC) - before;
    }
    for (int i = 0; i < count; i++) {
        pthread_join(channels[i].thread, NULL);
        reads += channels[i].reads; writes += channels[i].writes;
    }
    uint64_t elapsed = now(CLOCK_MONOTONIC) - start;
    cpu = now(CLOCK_PROCESS_CPUTIME_ID) - cpu;
    qsort(durations, (size_t)frames, sizeof(*durations), compare);
    printf("{\"wall_ns\":%llu,\"cpu_ns\":%llu,\"write_p50_ns\":%llu,\"write_p99_ns\":%llu,\"read_calls\":%llu,\"write_operations\":%llu,\"pipe_capacity\":%d}\n",
        (unsigned long long)elapsed, (unsigned long long)cpu,
        (unsigned long long)durations[(frames - 1) / 2],
        (unsigned long long)durations[frames - 1 - frames / 100],
        (unsigned long long)reads, (unsigned long long)writes,
        channels[0].tcp ? -1 : fcntl(channels[0].writer, F_GETPIPE_SZ));
}

int main(int argc, char **argv) {
    assert(argc == 7);
    int use_uring = atoi(argv[1]), tcp = atoi(argv[2]), count = atoi(argv[3]);
    size_t size = (size_t)strtoul(argv[4], NULL, 10);
    int frames = atoi(argv[5]), paced = atoi(argv[6]);
    assert(count >= 1 && count <= MAX_CHANNELS);
    assert(size >= 8 && size <= 32 * 1024 * 1024);
    assert(frames >= 1 && frames <= MAX_FRAMES);
    struct io_uring ring;
    if (use_uring) {
        int result = io_uring_queue_init(16, &ring, 0);
        if (result) { fprintf(stderr, "io_uring unavailable: %s\n", strerror(-result)); return 77; }
    }
    struct channel channels[MAX_CHANNELS];
    for (int i = 0; i < count; i++) initialize(&channels[i], tcp, size, frames);
    run(channels, count, use_uring ? &ring : NULL, paced);
    destroy(channels, count);
    if (use_uring) io_uring_queue_exit(&ring);
    return 0;
}
