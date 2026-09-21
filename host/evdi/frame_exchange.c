#define _GNU_SOURCE
#include "frame_exchange.h"
#include "pixel_damage.h"
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <errno.h>
#include <stdint.h>
#include <unistd.h>
#include <sys/eventfd.h>

void frame_exchange_init(frame_exchange_t *frames) {
    frames->demand_fd = -1;
    frames->reader_connected = frames->refresh_needed = 0;
    pthread_condattr_t attributes;
    pthread_condattr_init(&attributes);
    pthread_condattr_setclock(&attributes, CLOCK_MONOTONIC);
    pthread_cond_init(&frames->ready, &attributes);
    pthread_condattr_destroy(&attributes);
}

int frame_exchange_enable_demand(frame_exchange_t *frames) {
    if (frames->demand_fd >= 0) return 1;
    frames->demand_fd = eventfd(0, EFD_NONBLOCK | EFD_CLOEXEC);
    return frames->demand_fd >= 0;
}

void frame_exchange_reader(frame_exchange_t *frames, int connected) {
    if (frames->demand_fd < 0) return;
    pthread_mutex_lock(&frames->mutex);
    if (frames->reader_connected != connected) {
        frames->reader_connected = connected;
        frames->refresh_needed = connected;
        frames->latest_valid = 0;
        frames->generation++;
        uint64_t hint = 1;
        /* EAGAIN means an unread hint already exists. The mutex protects state;
         * the eventfd is only a wakeup, never the authority on reader demand. */
        while (write(frames->demand_fd, &hint, sizeof(hint)) < 0 && errno == EINTR) {}
    }
    pthread_mutex_unlock(&frames->mutex);
}

int frame_exchange_take_request(frame_exchange_t *frames) {
    if (frames->demand_fd < 0) return 0;
    uint64_t hint;
    while (read(frames->demand_fd, &hint, sizeof(hint)) < 0 && errno == EINTR) {}
    pthread_mutex_lock(&frames->mutex);
    int refresh = frames->reader_connected && frames->refresh_needed;
    pthread_mutex_unlock(&frames->mutex);
    return refresh;
}

int frame_exchange_begin(frame_exchange_t *frames, unsigned *generation) {
    pthread_mutex_lock(&frames->mutex);
    int wanted = frames->demand_fd < 0 || frames->reader_connected;
    if (wanted && frames->refresh_needed) frame_exchange_mark_all(frames);
    *generation = frames->generation;
    pthread_mutex_unlock(&frames->mutex);
    return wanted;
}

static void full_spans(pixel_span_t *spans, int rows, int width) {
    if (!spans) return;
    for (int cy = 0; cy < rows; cy++) spans[cy] = (pixel_span_t){0, width};
}

void frame_exchange_mark_all(frame_exchange_t *frames) {
    if (!frames->dirty_fill) return;
    memset(frames->dirty_fill,   0xFF, (size_t)frames->dirty_bytes);
    memset(frames->dirty_latest, 0xFF, (size_t)frames->dirty_bytes);
    memset(frames->dirty_write,  0xFF, (size_t)frames->dirty_bytes);
    full_spans(frames->spans_fill, frames->chroma_rows, frames->width);
    full_spans(frames->spans_latest, frames->chroma_rows, frames->width);
    full_spans(frames->spans_write, frames->chroma_rows, frames->width);
}

static void mark_region(frame_exchange_t *frames, pixel_span_t rows, pixel_span_t x) {
    pixel_damage_region(frames->dirty_fill, frames->spans_fill, rows, x);
    pixel_damage_region(frames->dirty_latest, frames->spans_latest, rows, x);
    pixel_damage_region(frames->dirty_write, frames->spans_write, rows, x);
}

void frame_exchange_damage(frame_exchange_t *frames, int y0, int y1, int scale) {
    if (!frames->dirty_fill || scale < 1 || scale > 4) return;
    pixel_span_t rows = pixel_chroma_range(y0, y1, scale, frames->chroma_rows);
    mark_region(frames, rows, (pixel_span_t){0, frames->width});
}

void frame_exchange_damage_rect(frame_exchange_t *frames, int x0, int y0, int x1, int y1, int scale) {
    if (!frames->dirty_fill || scale < 1 || scale > 4) return;
    pixel_span_t x = pixel_chroma_range(x0, x1, scale, frames->width / 2);
    if (x.begin == x.end) return;
    pixel_span_t rows = pixel_chroma_range(y0, y1, scale, frames->chroma_rows);
    mark_region(frames, rows, (pixel_span_t){2 * x.begin, 2 * x.end});
}

static int histories_allocated(const frame_exchange_t *frames) {
    return frames->dirty_fill && frames->dirty_latest && frames->dirty_write
        && frames->spans_fill && frames->spans_latest && frames->spans_write;
}

void frame_exchange_resize(frame_exchange_t *frames, int width, int height) {
    frames->width = width;
    frames->height = height;
    /* Packed buffers hold NV12 (Y plane + half-size interleaved CbCr). */
    frames->size = frames->width * frames->height * 3 / 2;
    free(frames->fill);   frames->fill = malloc(frames->size);
    free(frames->latest); frames->latest = malloc(frames->size);
    free(frames->write);  frames->write = malloc(frames->size);

    /* Fresh buffers hold nothing, so every row is stale in all of them. */
    frames->chroma_rows = frames->height / 2;
    frames->dirty_bytes = (frames->chroma_rows + 7) / 8;
    free(frames->dirty_fill);   frames->dirty_fill   = malloc((size_t)frames->dirty_bytes);
    free(frames->dirty_latest); frames->dirty_latest = malloc((size_t)frames->dirty_bytes);
    free(frames->dirty_write);  frames->dirty_write  = malloc((size_t)frames->dirty_bytes);
    size_t spans_size = (size_t)frames->chroma_rows * sizeof(pixel_span_t);
    free(frames->spans_fill);   frames->spans_fill = malloc(spans_size);
    free(frames->spans_latest); frames->spans_latest = malloc(spans_size);
    free(frames->spans_write);  frames->spans_write = malloc(spans_size);
    if (histories_allocated(frames)) frame_exchange_mark_all(frames);
}

int frame_exchange_allocated(const frame_exchange_t *frames) {
    return frames->fill && frames->latest && frames->write && histories_allocated(frames);
}

int frame_exchange_retire(frame_exchange_t *frames) {
    struct timespec deadline;
    clock_gettime(CLOCK_MONOTONIC, &deadline);
    deadline.tv_sec++;
    pthread_mutex_lock(&frames->mutex);
    frames->latest_valid = 0;
    frames->buffers_ready = 0;
    frames->generation++;
    while (frames->writer_busy) {
        int result = pthread_cond_timedwait(&frames->ready, &frames->mutex, &deadline);
        if (result != 0) break;
    }
    int released = !frames->writer_busy;
    pthread_mutex_unlock(&frames->mutex);
    return released;
}

void frame_exchange_publish(frame_exchange_t *frames, long long grabbed_us, unsigned generation) {
    pthread_mutex_lock(&frames->mutex);
    if (frames->generation != generation) {
        pthread_mutex_unlock(&frames->mutex);
        return;
    }
    memset(frames->dirty_fill, 0, (size_t)frames->dirty_bytes);
    unsigned char *data = frames->latest;
    frames->latest = frames->fill;
    frames->fill = data;
    unsigned char *dirty = frames->dirty_latest;
    frames->dirty_latest = frames->dirty_fill;
    frames->dirty_fill = dirty;
    pixel_span_t *spans = frames->spans_latest;
    frames->spans_latest = frames->spans_fill;
    frames->spans_fill = spans;
    frames->latest_valid = 1;
    frames->refresh_needed = 0;
    frames->latest_grab_us = grabbed_us;
    pthread_cond_signal(&frames->ready);
    pthread_mutex_unlock(&frames->mutex);
}

static void wait_for_frame(frame_exchange_t *frames, const atomic_int *running,
                           const struct timespec *deadline) {
    while (atomic_load(running) && (!frames->buffers_ready || !frames->latest_valid)) {
        /* Spurious wakes keep the same absolute deadline. With no cached frame
           only publication or shutdown can make progress: wait on the event. */
        int result = deadline ? pthread_cond_timedwait(&frames->ready, &frames->mutex, deadline)
                              : pthread_cond_wait(&frames->ready, &frames->mutex);
        if (result == ETIMEDOUT) break;
    }
}

int frame_exchange_claim(frame_exchange_t *frames, frame_cursor_t *cursor,
                         const atomic_int *running, const struct timespec *deadline, frame_lease_t *lease) {
    pthread_mutex_lock(&frames->mutex);
    wait_for_frame(frames, running, deadline);
    if (!atomic_load(running)) {
        pthread_mutex_unlock(&frames->mutex);
        return -1;
    }
    if (cursor->generation != frames->generation) {
        /* Buffers were reallocated; previous frames->write content is gone */
        cursor->generation = frames->generation;
        cursor->have_frame = 0;
    }
    if (!frames->buffers_ready) {
        pthread_mutex_unlock(&frames->mutex);
        return 0;
    }
    lease->fresh = 0;
    if (frames->latest_valid) {
        unsigned char *tmp = frames->write;
        frames->write = frames->latest;
        frames->latest = tmp;
        unsigned char *dtmp = frames->dirty_write;
        frames->dirty_write = frames->dirty_latest;
        frames->dirty_latest = dtmp;
        pixel_span_t *stmp = frames->spans_write;
        frames->spans_write = frames->spans_latest;
        frames->spans_latest = stmp;
        frames->latest_valid = 0;
        frames->write_grab_us = frames->latest_grab_us;
        cursor->have_frame = 1;
        lease->fresh = 1;
    }
    lease->data = frames->write;
    lease->size = (size_t)frames->size;
    lease->generation = cursor->generation;
    lease->grabbed_us = frames->write_grab_us;
    frames->writer_busy = cursor->have_frame;
    pthread_mutex_unlock(&frames->mutex);

    return cursor->have_frame;
}

void frame_exchange_release(frame_exchange_t *frames) {
    pthread_mutex_lock(&frames->mutex);
    frames->writer_busy = 0;
    /* A writer can also be waiting on this condition during retirement. Wake
       every predicate owner so publication/shutdown cannot consume a release. */
    pthread_cond_broadcast(&frames->ready);
    pthread_mutex_unlock(&frames->mutex);
}

void frame_exchange_free(frame_exchange_t *frames) {
    if (frames->demand_fd >= 0) close(frames->demand_fd);
    frames->demand_fd = -1;
    free(frames->fill); frames->fill = NULL;
    free(frames->latest); frames->latest = NULL;
    free(frames->write); frames->write = NULL;
    free(frames->dirty_fill); frames->dirty_fill = NULL;
    free(frames->dirty_latest); frames->dirty_latest = NULL;
    free(frames->dirty_write); frames->dirty_write = NULL;
    free(frames->spans_fill); frames->spans_fill = NULL;
    free(frames->spans_latest); frames->spans_latest = NULL;
    free(frames->spans_write); frames->spans_write = NULL;
}
