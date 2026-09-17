#define _GNU_SOURCE
#include "writer.h"
#include <time.h>
#include <limits.h>

/* Keep idle input for decoder watchdogs and CLI wall-clock IDR scheduling. */
#define IDLE_KEEPALIVE_MS 200
static long long writer_now_us(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long long)ts.tv_sec * 1000000 + ts.tv_nsec / 1000;
}
static long long writer_now_ms(void) { return writer_now_us() / 1000; }

static void record_latency(writer_context_t *writer, long long grab_us) {
    if (grab_us <= 0) return;
    long long d = writer_now_us() - grab_us;
    pthread_mutex_lock(&writer->frames->mutex);
    if (d >= 0 && d < 1000000 && writer->frames->latency_count < LAT_SAMPLES)
        writer->frames->latency[writer->frames->latency_count++] = (int)d;
    pthread_mutex_unlock(&writer->frames->mutex);
}

typedef struct {
    long period_ns;
    long long last_write_ms;
    struct timespec next_allowed;
    frame_cursor_t frame;
    frame_lease_t lease;
} writer_state_t;

static void add_period(struct timespec *time, long period_ns) {
    time->tv_nsec += period_ns;
    while (time->tv_nsec >= 1000000000L) {
        time->tv_nsec -= 1000000000L;
        time->tv_sec += 1;
    }
}

static int ensure_writer_fifo(writer_context_t *writer) {
    if (writer->fifo->fd < 0) {
        writer->fifo->fd = fifo_writer_open(writer->fifo);
        if (writer->fifo->fd < 0) {
            /* No reader yet: poll slowly instead of spinning. */
            struct timespec idle = { .tv_sec = 0, .tv_nsec = 50000000L };
            nanosleep(&idle, NULL);
            return 0;
        }
    }
    return 1;
}

/* The exchange owns locking and publishes a generation-tagged lease. */
static int claim_writer_frame(writer_context_t *writer, writer_state_t *state, int *size, int *fresh) {
    int result = frame_exchange_claim(writer->frames, &state->frame, writer->running,
                                      state->period_ns, &state->lease);
    if (result > 0) {
        *size = (int)state->lease.size;
        *fresh = state->lease.fresh;
    }
    return result;
}

static int writer_frame_due(writer_context_t *writer, writer_state_t *state, int fresh) {
    /* Nothing changed on screen: don't re-send the identical frame.
       A motionless desktop was still pushing 60 full NV12 frames a second
       through the FIFO — 8.2MB each, roughly half a gigabyte per second of
       pure memory traffic, plus an encode for every one of them, all to
       transmit no new information. The occasional keepalive keeps the
       encoder and the client's read timeout alive. */
    long long now_ms_write = writer_now_ms();
    if (!fresh && (now_ms_write - state->last_write_ms) < IDLE_KEEPALIVE_MS) {
        pthread_mutex_lock(&writer->frames->mutex);
        frame_exchange_release(writer->frames);
        pthread_mutex_unlock(&writer->frames->mutex);
        return 0;
    }
    state->last_write_ms = now_ms_write;

    return 1;
}

static void pace_writer(writer_state_t *state) {
    /* Rate limit against an absolute schedule, never against "now".
       Rebasing on now would add the wait and write time to every period,
       so the stream drifts slower than the target — measured as 58fps
       against a 60fps target, with ffmpeg reporting speed=0.97x. */
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    long long ahead_ns = (long long)(state->next_allowed.tv_sec - now.tv_sec) * 1000000000LL
                       + (state->next_allowed.tv_nsec - now.tv_nsec);
    if (ahead_ns > 0) {
        struct timespec gap = { .tv_sec = ahead_ns / 1000000000LL,
                                .tv_nsec = ahead_ns % 1000000000LL };
        nanosleep(&gap, NULL);
    } else if (-ahead_ns > 4 * (long long)state->period_ns) {
        /* Fallen far behind (a stalled encoder, a mode change): resync
           instead of trying to catch up on a burst of stale frames. */
        state->next_allowed = now;
    }
    add_period(&state->next_allowed, state->period_ns);
}

void *writer_run(void *arg) {
    writer_context_t *writer = arg;
    writer_state_t state = {
        .period_ns = 1000000000L / (writer->fps > 0 ? writer->fps : 60),
        .frame = {.generation = UINT_MAX},
    };
    clock_gettime(CLOCK_MONOTONIC, &state.next_allowed);
    while ((*writer->running)) {
        if (!ensure_writer_fifo(writer)) continue;
        int size, fresh;
        int claimed = claim_writer_frame(writer, &state, &size, &fresh);
        if (claimed < 0) break;
        if (claimed == 0) continue;
        if (!writer_frame_due(writer, &state, fresh)) continue;
        pace_writer(&state);
        size_t remaining = fifo_writer_write(writer->fifo, state.lease.data, (size_t)size, state.lease.generation);
        frame_exchange_release(writer->frames);
        /* Repeated keepalives measure stale frame age, not capture latency. */
        if (fresh && remaining == 0) record_latency(writer, state.lease.grabbed_us);
    }
    return NULL;
}
