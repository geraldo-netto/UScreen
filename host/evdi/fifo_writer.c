#define _GNU_SOURCE
#include "fifo_writer.h"
#include <stdio.h>
#include <unistd.h>
#include <fcntl.h>
#include <poll.h>
#include <errno.h>
#include <stdint.h>
#include <time.h>

static long long fifo_now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long long)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

static void release_retired_fifo(fifo_writer_t *fifo) {
    if (fifo->retired_fd >= 0) close(fifo->retired_fd);
    fifo->retired_fd = -1;
}

/* (Re)open the capture FIFO without blocking forever: O_NONBLOCK open fails
   with ENXIO while no reader (ffmpeg) has the other end open. */
int fifo_writer_open(fifo_writer_t *fifo) {
    /* O_NOFOLLOW: the path is in a private directory now, but a symlink
       planted there must still never redirect the screen into a file. */
    int fd = open(fifo->path, O_WRONLY | O_NONBLOCK | O_NOFOLLOW);
    if (fd < 0)
        return -1;
    /* Validate the opened object, so a mistaken path cannot overwrite a
       regular file and a path replacement cannot bypass the type check. */
    struct stat info;
    if (fstat(fd, &info) != 0 || !S_ISFIFO(info.st_mode)) {
        close(fd);
        return -1;
    }
    if (fifo->retired && info.st_dev == fifo->retired_device &&
            info.st_ino == fifo->retired_inode) {
        close(fd);
        return -1;
    }
    release_retired_fifo(fifo);
    fifo->retired = 0;
    /* Keep writes nonblocking: POLLOUT promises some space, not enough for
       an entire frame. The writer owns this fd until it closes/reopens it. */
    /* Enlarge the pipe so a full-frame write doesn't take hundreds of
       64KB round-trips with the encoder. */
    fcntl(fd, F_SETPIPE_SZ, 1 << 20);
    fprintf(stderr, "[evdi-helper] Capture FIFO opened\n");
    return fd;
}

/* Writer thread: sends the most recent frame, woken by the grabber rather than
   by a timer, and rate-limited so it never exceeds the target fps.

   The previous version free-ran on clock_nanosleep, entirely independent of
   when a frame was actually grabbed. A frame published just after a tick had to
   wait a whole period before being sent — half a frame of pure added latency on
   average, for nothing. Waiting on the condition variable removes that: the
   frame goes out as soon as it exists, and the minimum-interval check below
   still caps the rate.

   FIFO backpressure never stalls capture, and the FIFO is reopened
   automatically when the encoder restarts. */
enum fifo_wait_result { FIFO_STOP, FIFO_RETRY, FIFO_READY };

static enum fifo_wait_result wait_fifo_writable(fifo_writer_t *fifo, long long deadline) {
    long long wait_ms = deadline - fifo_now_ms();
    if (wait_ms <= 0) return FIFO_STOP;
    struct pollfd wfd = { .fd = fifo->fd, .events = POLLOUT };
    int pr = poll(&wfd, 1, wait_ms < 250 ? (int)wait_ms : 250);
    if (pr < 0 && errno == EINTR) return FIFO_RETRY;
    if (pr == 0 && fifo_now_ms() < deadline) return FIFO_RETRY;
    if (pr <= 0 || (wfd.revents & (POLLERR | POLLHUP | POLLNVAL))) return FIFO_STOP;
    return FIFO_READY;
}

static int fifo_write_retryable(ssize_t written) {
    return written < 0 && (errno == EINTR || errno == EAGAIN);
}

/* A live reader may stall briefly under load. Keep the same frame across
   poll timeouts, but bound a continuous stall and notice mode changes. */
static size_t write_fifo_bytes(fifo_writer_t *fifo, const unsigned char *ptr, size_t remaining, unsigned generation) {
    long long deadline = fifo_now_ms() + 1000;
    while (remaining > 0 && (*fifo->running) && generation == (*fifo->generation)) {
        enum fifo_wait_result ready = wait_fifo_writable(fifo, deadline);
        if (ready == FIFO_RETRY) continue;
        if (ready == FIFO_STOP) break;
        ssize_t written = write(fifo->fd, ptr, remaining);
        if (written <= 0) {
            if (fifo_write_retryable(written)) continue;
            break;
        }
        ptr += written;
        remaining -= (size_t)written;
        deadline = fifo_now_ms() + 1000;
    }
    return remaining;
}

/* Tell the supervisor which reader must be retired. It replaces the FIFO
   while preserving this helper and the attached virtual display. */
static void retire_partial_fifo(fifo_writer_t *fifo) {
    struct stat info;
    if (fstat(fifo->fd, &info) != 0) {
        fprintf(stderr, "[evdi-helper] Cannot identify damaged FIFO; stopping capture\n");
        (*fifo->running) = 0;
        return;
    }
    release_retired_fifo(fifo);
    fifo->retired_fd = fifo->fd;
    fifo->fd = -1;
    fifo->retired = 1;
    fifo->retired_device = info.st_dev;
    fifo->retired_inode = info.st_ino;
    printf("FIFO_RESET %ju %ju\n", (uintmax_t)info.st_dev, (uintmax_t)info.st_ino);
    fflush(stdout);
}

size_t fifo_writer_write(fifo_writer_t *fifo, const unsigned char *ptr, size_t remaining, unsigned generation) {
    /* Keep the generation from claim_writer_frame, including across pacing.
       An obsolete frame that has not started needs no FIFO resynchronization;
       completed earlier frames remain intact. Shutdown still closes the pipe. */
    if (generation != (*fifo->generation) && (*fifo->running)) return remaining;
    size_t size = remaining;
    remaining = write_fifo_bytes(fifo, ptr, remaining, generation);
    if (remaining > 0) {
        if (remaining < size) retire_partial_fifo(fifo);
        fprintf(stderr, "[evdi-helper] Incomplete frame — retiring FIFO writer\n");
        if (fifo->fd >= 0) close(fifo->fd);
        fifo->fd = -1;
    }
    return remaining;
}

void fifo_writer_close(fifo_writer_t *fifo) {
    if (fifo->fd >= 0) close(fifo->fd);
    fifo->fd = -1;
    release_retired_fifo(fifo);
}
