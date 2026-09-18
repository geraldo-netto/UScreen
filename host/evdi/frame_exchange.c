#define _GNU_SOURCE
#include "frame_exchange.h"
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <errno.h>
#include <stdint.h>
#include <unistd.h>

void frame_exchange_init(frame_exchange_t *frames) {
    pthread_condattr_t attributes;
    pthread_condattr_init(&attributes);
    pthread_condattr_setclock(&attributes, CLOCK_MONOTONIC);
    pthread_cond_init(&frames->ready, &attributes);
    pthread_condattr_destroy(&attributes);
}

void frame_exchange_mark_all(frame_exchange_t *frames) {
    if (!frames->dirty_fill) return;
    memset(frames->dirty_fill,   0xFF, (size_t)frames->dirty_bytes);
    memset(frames->dirty_latest, 0xFF, (size_t)frames->dirty_bytes);
    memset(frames->dirty_write,  0xFF, (size_t)frames->dirty_bytes);
}

/* OR one bit range without revisiting every row of overlapping rectangles. */
static void mark_range(unsigned char *mask, int first, int end) {
    if (first >= end) return;
    int begin_byte = first / 8, last_byte = (end - 1) / 8;
    unsigned char left = (unsigned char)(0xFFu << (first & 7));
    unsigned char right = (unsigned char)(0xFFu >> (7 - ((end - 1) & 7)));
    if (begin_byte == last_byte) { mask[begin_byte] |= left & right; return; }
    mask[begin_byte] |= left;
    memset(mask + begin_byte + 1, 0xFF, (size_t)(last_byte - begin_byte - 1));
    mask[last_byte] |= right;
}

void frame_exchange_damage(frame_exchange_t *frames, int y0, int y1, int scale) {
    if (!frames->dirty_fill || frames->chroma_rows <= 0) return;
    if (y1 < y0) { int t = y0; y0 = y1; y1 = t; }
    /* Source rows map onto output chroma rows through the scale: one
       chroma row covers 2*scale source rows. */
    int div = 2 * scale;
    /* The driver may report an out-of-frame endpoint. Round in a wider
       type before clipping so INT_MAX cannot overflow into a negative row. */
    int c0 = y0 / div, c1 = (int)(((int64_t)y1 + div - 1) / div);
    if (c0 < 0) c0 = 0;
    if (c1 > frames->chroma_rows) c1 = frames->chroma_rows;
    mark_range(frames->dirty_fill, c0, c1);
    mark_range(frames->dirty_latest, c0, c1);
    mark_range(frames->dirty_write, c0, c1);
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
    if (frames->dirty_fill && frames->dirty_latest && frames->dirty_write)
        frame_exchange_mark_all(frames);
}

int frame_exchange_allocated(const frame_exchange_t *frames) {
    return frames->fill && frames->latest && frames->write && frames->dirty_fill
        && frames->dirty_latest && frames->dirty_write;
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

void frame_exchange_publish(frame_exchange_t *frames, long long grabbed_us) {
    memset(frames->dirty_fill, 0, (size_t)frames->dirty_bytes);
    pthread_mutex_lock(&frames->mutex);
    unsigned char *data = frames->latest;
    frames->latest = frames->fill;
    frames->fill = data;
    unsigned char *dirty = frames->dirty_latest;
    frames->dirty_latest = frames->dirty_fill;
    frames->dirty_fill = dirty;
    frames->latest_valid = 1;
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
    free(frames->fill); frames->fill = NULL;
    free(frames->latest); frames->latest = NULL;
    free(frames->write); frames->write = NULL;
    free(frames->dirty_fill); frames->dirty_fill = NULL;
    free(frames->dirty_latest); frames->dirty_latest = NULL;
    free(frames->dirty_write); frames->dirty_write = NULL;
}
