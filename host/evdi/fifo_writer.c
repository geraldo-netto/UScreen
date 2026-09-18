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

/* The supervisor/GUI publishes one atomic, private, single-digit request.
 * Missing/malformed files retain the compatible 1 MiB default. */
static int capacity_value(const char *bytes, ssize_t count) {
    if (count == 2 && bytes[1] == '\n') count = 1;
    if (count != 1) return 1;
    switch (bytes[0]) {
        case '2': return 2;
        case '4': return 4;
        case '8': return 8;
        default: return 1;
    }
}

static int requested_capacity(const char *path) {
    if (!path) return 1 << 20;
    int fd = open(path, O_RDONLY | O_NONBLOCK | O_NOFOLLOW | O_CLOEXEC);
    if (fd < 0) return 1 << 20;
    struct stat info;
    char bytes[4];
    ssize_t count = -1;
    if (fstat(fd, &info) == 0 && S_ISREG(info.st_mode) && info.st_uid == geteuid())
        count = read(fd, bytes, sizeof(bytes));
    close(fd);
    return capacity_value(bytes, count) << 20;
}

static void report_capacity(fifo_writer_t *fifo, int fd, int requested, int effective, int error) {
    if (requested == fifo->capacity_requested && effective == fifo->capacity_effective &&
            error == fifo->capacity_error) return;
    fifo->capacity_requested = requested;
    fifo->capacity_effective = effective;
    fifo->capacity_error = error;
    fprintf(stderr, "[evdi-helper] Capture pipe requested=%d effective=%d bytes resize_errno=%d\n",
            requested, effective, error);
    struct stat info = {0};
    if (fd >= 0) (void)fstat(fd, &info);
    printf("PIPE_CAPACITY %d %d %d %ju %ju\n", requested, effective > 0 ? effective : 0,
           error, (uintmax_t)info.st_dev, (uintmax_t)info.st_ino);
    fflush(stdout);
}

/* Only the writer calls this, between whole frames or immediately on open.
 * Linux refuses an occupied shrink with EBUSY; leave all queued bytes intact
 * and retry after a second. Permission/limit failures retain the working pipe. */
static void update_capacity(fifo_writer_t *fifo, int fd, int force) {
    if (!force && !fifo->capacity_path) return;
    long long now = fifo_now_ms();
    if (!force && now - fifo->capacity_checked_ms < 1000) return;
    fifo->capacity_checked_ms = now;
    int requested = requested_capacity(fifo->capacity_path);
    int effective = fcntl(fd, F_GETPIPE_SZ);
    int error = 0;
    if ((force || effective != requested) && fcntl(fd, F_SETPIPE_SZ, requested) < 0) error = errno;
    effective = fcntl(fd, F_GETPIPE_SZ);
    if (effective < 0) error = errno;
    report_capacity(fifo, fd, requested, effective, error);
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
    /* Preserve the previous best-effort 1 MiB baseline if a larger request
       is refused on a newly opened pipe. Never shrink an existing larger one. */
    if (fcntl(fd, F_GETPIPE_SZ) < (1 << 20)) (void)fcntl(fd, F_SETPIPE_SZ, 1 << 20);
    fifo->capacity_effective = -1; /* New inode/open must publish even if sizes match. */
    update_capacity(fifo, fd, 1);
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

static enum fifo_wait_result retry_fifo_write(fifo_writer_t *fifo, ssize_t written, long long deadline) {
    if (written >= 0) return FIFO_STOP;
    if (errno == EINTR) return FIFO_RETRY;
    if (errno == EAGAIN) return wait_fifo_writable(fifo, deadline);
    return FIFO_STOP;
}

/* A live reader may stall briefly under load. Keep the same frame across
   poll timeouts, but bound a continuous stall and notice mode changes. */
static size_t write_fifo_bytes(fifo_writer_t *fifo, const unsigned char *ptr, size_t remaining, unsigned generation) {
    long long deadline = fifo_now_ms() + 1000;
    while (remaining > 0 && (*fifo->running) && generation == (*fifo->generation)) {
        ssize_t written = write(fifo->fd, ptr, remaining);
        if (written <= 0) {
            if (retry_fifo_write(fifo, written, deadline) == FIFO_STOP) break;
            continue;
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
    update_capacity(fifo, fifo->fd, 0);
    size_t size = remaining;
    remaining = write_fifo_bytes(fifo, ptr, remaining, generation);
    if (remaining > 0) {
        if (remaining < size) retire_partial_fifo(fifo);
        fprintf(stderr, "[evdi-helper] Incomplete frame — retiring FIFO writer\n");
        if (fifo->fd >= 0) close(fifo->fd);
        fifo->fd = -1;
        report_capacity(fifo, -1, 0, 0, 0);
    }
    return remaining;
}

void fifo_writer_close(fifo_writer_t *fifo) {
    if (fifo->fd >= 0) close(fifo->fd);
    fifo->fd = -1;
    release_retired_fifo(fifo);
    report_capacity(fifo, -1, 0, 0, 0);
}
