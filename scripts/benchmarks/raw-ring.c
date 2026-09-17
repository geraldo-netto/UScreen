/* T389 isolated raw transport prototype: stock Linux APIs, no EVDI/FFmpeg. */
#define _GNU_SOURCE
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdalign.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define SLOTS 3
#define WARMUP 32
#define HUGE (2u * 1024u * 1024u)
typedef struct { alignas(64) atomic_uint ready; } slot_t;
typedef struct { uint64_t seq, stamp; } message_t;
typedef struct {
    unsigned char *memory;
    size_t size, frame_size, stride, offset;
    int fd, allocated, advised;
    slot_t *slots;
} storage_t;
typedef struct { uint64_t cpu_ns; long faults, major, voluntary, involuntary; } counters_t;

static uint64_t clock_ns(clockid_t id) {
    struct timespec stamp;
    assert(clock_gettime(id, &stamp) == 0);
    return (uint64_t)stamp.tv_sec * 1000000000u + stamp.tv_nsec;
}
static counters_t counters(void) {
    struct rusage usage;
    assert(getrusage(RUSAGE_SELF, &usage) == 0);
    return (counters_t){clock_ns(CLOCK_PROCESS_CPUTIME_ID), usage.ru_minflt, usage.ru_majflt,
                        usage.ru_nvcsw, usage.ru_nivcsw};
}
static counters_t difference(counters_t before) {
    counters_t after = counters();
    return (counters_t){after.cpu_ns - before.cpu_ns, after.faults - before.faults,
        after.major - before.major, after.voluntary - before.voluntary, after.involuntary - before.involuntary};
}
static size_t rounded(size_t size, size_t unit) { return (size + unit - 1) / unit * unit; }
static storage_t layout(size_t size) {
    size_t page = (size_t)sysconf(_SC_PAGESIZE);
    size_t stride = rounded(size, page);
    return (storage_t){.size = rounded(page + SLOTS * stride, HUGE), .frame_size = size,
                      .stride = stride, .offset = page, .fd = -1};
}
static unsigned char *pixels(storage_t *store, uint64_t sequence) {
    return store->memory + store->offset + sequence % SLOTS * store->stride;
}
static void advise(storage_t *store, const char *allocation) {
    if (strcmp(allocation, "thp") == 0)
        store->advised = madvise(store->memory, store->size, MADV_HUGEPAGE) == 0;
    else
        assert(madvise(store->memory, store->size, MADV_NOHUGEPAGE) == 0);
}
static void seal_size(storage_t *store) {
    int seals = F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL;
    assert(fcntl(store->fd, F_ADD_SEALS, seals) == 0);
    assert((fcntl(store->fd, F_GET_SEALS) & seals) == seals);
    assert(ftruncate(store->fd, (off_t)store->size - 1) == -1 && errno == EPERM);
}
static storage_t allocate(size_t size, int ring, const char *allocation) {
    storage_t store = layout(size);
    if (ring) {
        store.fd = memfd_create("uscreen-t389-ring", MFD_CLOEXEC | MFD_ALLOW_SEALING);
        assert(store.fd >= 0);
        assert(ftruncate(store.fd, (off_t)store.size) == 0);
        store.memory = mmap(NULL, store.size, PROT_READ | PROT_WRITE, MAP_SHARED, store.fd, 0);
        assert(store.memory != MAP_FAILED);
        seal_size(&store);
    } else if (strcmp(allocation, "aligned") == 0) {
        store.size = rounded(store.size, HUGE);
        assert(posix_memalign((void **)&store.memory, HUGE, store.size) == 0);
        store.allocated = 1;
    } else {
        store.size = rounded(store.size, HUGE);
        store.memory = mmap(NULL, store.size, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        assert(store.memory != MAP_FAILED);
    }
    advise(&store, allocation);
    store.slots = (slot_t *)store.memory;
    for (int i = 0; i < SLOTS; i++) { atomic_init(&store.slots[i].ready, 0); assert(atomic_is_lock_free(&store.slots[i].ready)); }
    return store;
}
static void release_storage(storage_t *store) {
    if (store->allocated) free(store->memory);
    else assert(munmap(store->memory, store->size) == 0);
    if (store->fd >= 0) assert(close(store->fd) == 0);
}
static void send_descriptor(int socket, int fd) {
    char payload = 'R';
    struct iovec vector = {&payload, 1};
    union { struct cmsghdr aligned; char data[CMSG_SPACE(sizeof(int))]; } control = {0};
    struct msghdr message = {.msg_iov = &vector, .msg_iovlen = 1, .msg_control = control.data, .msg_controllen = sizeof(control)};
    struct cmsghdr *header = CMSG_FIRSTHDR(&message);
    header->cmsg_level = SOL_SOCKET; header->cmsg_type = SCM_RIGHTS; header->cmsg_len = CMSG_LEN(sizeof(int));
    memcpy(CMSG_DATA(header), &fd, sizeof(fd));
    assert(sendmsg(socket, &message, MSG_NOSIGNAL) == 1);
}
static int receive_descriptor(int socket) {
    char payload = 0;
    struct iovec vector = {&payload, 1};
    union { struct cmsghdr aligned; char data[CMSG_SPACE(sizeof(int))]; } control = {0};
    struct msghdr message = {.msg_iov = &vector, .msg_iovlen = 1, .msg_control = control.data, .msg_controllen = sizeof(control)};
    assert(recvmsg(socket, &message, MSG_CMSG_CLOEXEC) == 1);
    assert(payload == 'R' && !(message.msg_flags & (MSG_CTRUNC | MSG_TRUNC)));
    struct cmsghdr *header = CMSG_FIRSTHDR(&message);
    assert(header && header->cmsg_level == SOL_SOCKET && header->cmsg_type == SCM_RIGHTS);
    assert(header->cmsg_len == CMSG_LEN(sizeof(int)));
    int fd;
    memcpy(&fd, CMSG_DATA(header), sizeof(fd));
    return fd;
}
static storage_t receive_storage(int socket, size_t size) {
    storage_t store = layout(size);
    store.fd = receive_descriptor(socket);
    struct stat metadata;
    assert(fstat(store.fd, &metadata) == 0 && (size_t)metadata.st_size == store.size);
    int seals = F_SEAL_GROW | F_SEAL_SHRINK;
    assert((fcntl(store.fd, F_GET_SEALS) & seals) == seals);
    store.memory = mmap(NULL, store.size, PROT_READ | PROT_WRITE, MAP_SHARED, store.fd, 0);
    assert(store.memory != MAP_FAILED);
    /* Consumer may update control atomics, but its pixel mapping is read-only. */
    assert(mprotect(store.memory + store.offset, store.size - store.offset, PROT_READ) == 0);
    store.slots = (slot_t *)store.memory;
    return store;
}
static void send_bytes(int socket, const void *bytes, size_t size) {
    assert(send(socket, bytes, size, MSG_NOSIGNAL) == (ssize_t)size);
}
static void receive_bytes(int socket, void *bytes, size_t size) {
    assert(recv(socket, bytes, size, MSG_TRUNC) == (ssize_t)size);
}
static uint64_t transfer(int fd, unsigned char *bytes, size_t size, int writing) {
    uint64_t calls = 0;
    while (size) {
        ssize_t count = writing ? write(fd, bytes, size) : read(fd, bytes, size);
        calls++;
        if (count < 0 && errno == EINTR) continue;
        assert(count > 0);
        bytes += count; size -= (size_t)count;
    }
    return calls;
}
static void verify(const unsigned char *bytes, size_t size, unsigned char expected) {
    for (size_t offset = 0; offset < size; offset += 4096) assert(bytes[offset] == expected);
    assert(bytes[size - 1] == expected);
}
static unsigned long huge_kb(const storage_t *store) {
    FILE *file = fopen("/proc/self/smaps", "re");
    assert(file);
    char *line = NULL;
    size_t capacity = 0;
    unsigned long start, end, kb, total = 0;
    int matching = 0;
    while (getline(&line, &capacity, file) >= 0) {
        if (sscanf(line, "%lx-%lx", &start, &end) == 2)
            matching = start < (uintptr_t)store->memory + store->size && end > (uintptr_t)store->memory;
        else if (matching && (sscanf(line, "AnonHugePages: %lu", &kb) == 1 || sscanf(line, "ShmemPmdMapped: %lu", &kb) == 1)) total += kb;
    }
    free(line); fclose(file);
    return total;
}
static void await_release(int socket, storage_t *store, uint64_t sequence, int ring) {
    uint64_t released;
    receive_bytes(socket, &released, sizeof(released));
    assert(released == sequence);
    if (ring) assert(atomic_load_explicit(&store->slots[sequence % SLOTS].ready, memory_order_acquire) == 0);
}
static void publish(int socket, int pipe_fd, storage_t *store, uint64_t sequence, int ring) {
    message_t message = {sequence, clock_ns(CLOCK_MONOTONIC)};
    unsigned char *data = pixels(store, sequence);
    memset(data, (unsigned char)sequence, store->frame_size);
    if (ring) atomic_store_explicit(&store->slots[sequence % SLOTS].ready, 1, memory_order_release);
    send_bytes(socket, &message, sizeof(message));
    if (!ring) transfer(pipe_fd, data, store->frame_size, 1);
}
static void producer(int socket, int pipe_fd, size_t size, uint64_t count, int ring, const char *allocation) {
    counters_t allocation_start = counters();
    uint64_t touch_start = clock_ns(CLOCK_MONOTONIC);
    storage_t store = allocate(size, ring, allocation);
    memset(store.memory + store.offset, 0, store.size - store.offset);
    uint64_t touch_ns = clock_ns(CLOCK_MONOTONIC) - touch_start;
    counters_t touch_counts = difference(allocation_start);
    if (ring) send_descriptor(socket, store.fd);
    counters_t initial = counters();
    for (uint64_t seq = 0; seq < count + WARMUP; seq++) {
        if (seq >= SLOTS) await_release(socket, &store, seq - SLOTS, ring);
        if (seq == WARMUP) initial = counters();
        publish(socket, pipe_fd, &store, seq, ring);
    }
    for (uint64_t seq = count + WARMUP - SLOTS; seq < count + WARMUP; seq++) await_release(socket, &store, seq, ring);
    counters_t measured = difference(initial);
    uint64_t memory[] = {touch_ns, (uint64_t)touch_counts.faults, (uint64_t)touch_counts.major, huge_kb(&store), (uint64_t)store.advised};
    send_bytes(socket, &measured, sizeof(measured));
    send_bytes(socket, memory, sizeof(memory));
    release_storage(&store);
    close(socket); close(pipe_fd);
    _exit(0);
}
static void consume(int socket, int pipe_fd, storage_t *store, int ring, uint64_t sequence, uint64_t *calls, uint64_t *age) {
    message_t message;
    receive_bytes(socket, &message, sizeof(message));
    assert(message.seq == sequence);
    unsigned char *data = pixels(store, sequence);
    if (ring) assert(atomic_load_explicit(&store->slots[sequence % SLOTS].ready, memory_order_acquire) == 1);
    else *calls += transfer(pipe_fd, data, store->frame_size, 0);
    verify(data, store->frame_size, (unsigned char)sequence);
    *age = clock_ns(CLOCK_MONOTONIC) - message.stamp;
    if (ring) atomic_store_explicit(&store->slots[sequence % SLOTS].ready, 0, memory_order_release);
    send_bytes(socket, &sequence, sizeof(sequence));
}
static void report(counters_t measured, counters_t produced, uint64_t elapsed, uint64_t calls, const uint64_t *ages, uint64_t count, uint64_t *memory, int capacity) {
    printf("{\"ns\":%llu,\"cpu_ns\":%llu,\"reads\":%llu,\"consumer_minor_faults\":%ld,\"consumer_switches\":%ld,\"producer_minor_faults\":%ld,\"producer_switches\":%ld,\"touch_ns\":%llu,\"touch_minor_faults\":%llu,\"touch_major_faults\":%llu,\"huge_kb\":%llu,\"advice_accepted\":%llu,\"pipe_capacity\":%d,\"age_ns\":[",
        (unsigned long long)elapsed, (unsigned long long)(measured.cpu_ns + produced.cpu_ns), (unsigned long long)calls,
        measured.faults, measured.voluntary + measured.involuntary, produced.faults, produced.voluntary + produced.involuntary,
        (unsigned long long)memory[0], (unsigned long long)memory[1], (unsigned long long)memory[2],
        (unsigned long long)memory[3], (unsigned long long)memory[4], capacity);
    for (uint64_t i = 0; i < count; i++) printf("%s%llu", i ? "," : "", (unsigned long long)ages[i]);
    puts("]}");
}
static void consumer(int socket, int pipe_fd, size_t size, uint64_t count, int ring, int capacity) {
    storage_t store = ring ? receive_storage(socket, size) : allocate(size, 0, "aligned");
    uint64_t *ages = calloc(count, sizeof(*ages));
    assert(ages);
    uint64_t calls = 0, ignored;
    for (uint64_t seq = 0; seq < WARMUP; seq++) consume(socket, pipe_fd, &store, ring, seq, &calls, &ignored);
    counters_t initial = counters();
    uint64_t start = clock_ns(CLOCK_MONOTONIC);
    calls = 0;
    for (uint64_t seq = WARMUP; seq < count + WARMUP; seq++) consume(socket, pipe_fd, &store, ring, seq, &calls, &ages[seq - WARMUP]);
    uint64_t elapsed = clock_ns(CLOCK_MONOTONIC) - start;
    counters_t measured = difference(initial), produced;
    uint64_t memory[5];
    receive_bytes(socket, &produced, sizeof(produced));
    receive_bytes(socket, memory, sizeof(memory));
    report(measured, produced, elapsed, calls, ages, count, memory, capacity);
    free(ages); release_storage(&store);
}
int main(int argc, char **argv) {
    assert(argc == 5);
    size_t size = strtoull(argv[1], NULL, 10);
    uint64_t count = strtoull(argv[2], NULL, 10);
    int ring = strcmp(argv[3], "ring") == 0;
    assert(size > 0 && size <= 64 * 1024 * 1024 && count >= SLOTS && count <= 10000);
    assert(ring || strcmp(argv[3], "fifo") == 0);
    alarm(60);
    int sockets[2], pipes[2];
    assert(socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0, sockets) == 0);
    assert(pipe2(pipes, O_CLOEXEC) == 0);
    fcntl(pipes[0], F_SETPIPE_SZ, 1024 * 1024);
    int capacity = fcntl(pipes[0], F_GETPIPE_SZ);
    pid_t child = fork();
    assert(child >= 0);
    if (child == 0) {
        close(sockets[0]); close(pipes[0]);
        producer(sockets[1], pipes[1], size, count, ring, argv[4]);
    }
    close(sockets[1]); close(pipes[1]);
    consumer(sockets[0], pipes[0], size, count, ring, capacity);
    close(sockets[0]); close(pipes[0]);
    int status;
    assert(waitpid(child, &status, 0) == child);
    assert(WIFEXITED(status) && WEXITSTATUS(status) == 0);
    return 0;
}
