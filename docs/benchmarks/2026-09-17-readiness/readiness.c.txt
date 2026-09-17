/* T405: instrument the actual revision's native FIFO writer. */
#define _GNU_SOURCE
#include <unistd.h>
#include <poll.h>
#include <assert.h>
static _Thread_local unsigned long long writes, polls;
static ssize_t counted_write(int fd, const void *bytes, size_t size) {
    writes++;
    return write(fd, bytes, size);
}
static int counted_poll(struct pollfd *fds, nfds_t count, int timeout) {
    polls++;
    return poll(fds, count, timeout);
}
#define write counted_write
#define poll counted_poll
#include "fifo_writer.c"
#undef poll
#undef write
void bench_write(int fd, const unsigned char *bytes, size_t size) {
    atomic_int running = 1;
    atomic_uint generation = 1;
    fifo_writer_t fifo = FIFO_WRITER_INITIALIZER(&running, &generation);
    fifo.fd = fd;
    assert(fifo_writer_write(&fifo, bytes, size, 1) == 0);
}
void bench_counts(unsigned long long *counts) {
    counts[0] = writes;
    counts[1] = polls;
}
