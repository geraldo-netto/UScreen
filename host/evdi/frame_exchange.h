#ifndef BLENT_FRAME_EXCHANGE_H
#define BLENT_FRAME_EXCHANGE_H
#include <pthread.h>
#include <stdatomic.h>
#include <stddef.h>
#include <time.h>
#include "pixel_span.h"

#define LAT_SAMPLES 256
/* Buffer, dirty-mask and span pointers always travel together. A span is valid
 * only while its row's dirty bit is set. The capture owner alone converts fill;
 * the writer may borrow write until release(). */
typedef struct {
    pthread_mutex_t mutex;
    pthread_cond_t ready;
    unsigned char *fill, *latest, *write;
    unsigned char *dirty_fill, *dirty_latest, *dirty_write;
    pixel_span_t *spans_fill, *spans_latest, *spans_write;
    int width, height, size, chroma_rows, dirty_bytes;
    int buffers_ready, latest_valid;
    int demand_fd, reader_connected, refresh_needed;
    atomic_int writer_busy;
    atomic_uint generation;
    long long latest_grab_us, write_grab_us;
    int latency[LAT_SAMPLES], latency_count;
} frame_exchange_t;
#define FRAME_EXCHANGE_INITIALIZER { .mutex = PTHREAD_MUTEX_INITIALIZER, .demand_fd = -1 }

typedef struct { int have_frame; unsigned generation; } frame_cursor_t;
typedef struct {
    const unsigned char *data;
    size_t size;
    unsigned generation;
    long long grabbed_us;
    int fresh;
} frame_lease_t;

void frame_exchange_init(frame_exchange_t *frames);
/* FIFO only, before writer startup. Failure leaves legacy conversion enabled. */
int frame_exchange_enable_demand(frame_exchange_t *frames);
/* Writer only, without an active lease. Invalidates cached frames on transitions. */
void frame_exchange_reader(frame_exchange_t *frames, int connected);
/* Capture owner: drain the wakeup hint and check whether a full refresh is due. */
int frame_exchange_take_request(frame_exchange_t *frames);
/* Capture owner: snapshot demand/generation before converting; zero skips work. */
int frame_exchange_begin(frame_exchange_t *frames, unsigned *generation);
/* Invalidate generation, then wait boundedly for the writer's last lease.
 * On failure every old allocation must survive until the writer is joined. */
int frame_exchange_retire(frame_exchange_t *frames);
/* Capture thread only: retire() must have succeeded; dimensions are validated
 * positive/even and size-safe by the mode owner before this call. */
void frame_exchange_resize(frame_exchange_t *frames, int width, int height);
int frame_exchange_allocated(const frame_exchange_t *frames);
/* Call damage routines with mutex held once a writer exists. */
void frame_exchange_mark_all(frame_exchange_t *frames);
void frame_exchange_damage(frame_exchange_t *frames, int y0, int y1, int scale);
/* Source rectangle; normalizes reversed endpoints, clips bounds and includes
 * complete scaled chroma blocks. Empty/off-frame rectangles do no work. */
void frame_exchange_damage_rect(frame_exchange_t *frames, int x0, int y0, int x1, int y1, int scale);
/* Publish only if the conversion still belongs to this reader/mode generation. */
void frame_exchange_publish(frame_exchange_t *frames, long long grabbed_us, unsigned generation);
/* Absolute CLOCK_MONOTONIC keepalive deadline, or NULL to wait for an event.
 * -1 stopped, 0 no frame, 1 immutable lease, always followed by release(). */
int frame_exchange_claim(frame_exchange_t *frames, frame_cursor_t *cursor,
                         const atomic_int *running, const struct timespec *deadline, frame_lease_t *lease);
/* Owns locking; call without holding mutex. Notifies retirement waiters. */
void frame_exchange_release(frame_exchange_t *frames);
/* Only after writer join and conversion completion. */
void frame_exchange_free(frame_exchange_t *frames);
#endif
