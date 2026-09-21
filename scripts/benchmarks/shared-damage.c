/* T570: exact capture publication code, isolated from EVDI and encoding.
 * Link with --gc-sections; unused device callbacks are never called. */
#include "capture.c"
#include <assert.h>
#include <sys/socket.h>
#include <inttypes.h>

enum { WIDTH = 1280, HEIGHT = 800, SAMPLES = 400 };
typedef struct {
    raw_ring_t ring;
    frame_exchange_t frames;
    conv_pool_t pool;
    capture_context_t capture;
    atomic_int running;
    int peer, scale, mode;
    uint64_t hash;
} replay_t;

static uint64_t now_ns(clockid_t clock) {
    struct timespec ts; assert(clock_gettime(clock, &ts) == 0);
    return (uint64_t)ts.tv_sec * 1000000000 + ts.tv_nsec;
}

static void initialize(replay_t *r) {
    r->ring = (raw_ring_t)RAW_RING_INITIALIZER;
    r->ring.nonce = 570;
    int pair[2]; assert(socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_NONBLOCK, 0, pair) == 0);
    r->peer = pair[1];
    int w = (WIDTH / r->scale) & ~1, h = (HEIGHT / r->scale) & ~1;
    assert(raw_ring_init(&r->ring, pair[0]) && raw_ring_resize(&r->ring, w, h));
    r->frames = (frame_exchange_t)FRAME_EXCHANGE_INITIALIZER;
    frame_exchange_init(&r->frames);
    r->frames.width = w; r->frames.height = h; r->frames.buffers_ready = 1;
    r->pool = (conv_pool_t)CONV_POOL_INITIALIZER;
    conv_pool_start(&r->pool, 8);
    r->capture = (capture_context_t)CAPTURE_INITIALIZER(&r->frames, &r->pool, &r->running);
    r->capture.mode_w = WIDTH; r->capture.mode_h = HEIGHT; r->capture.mode_stride = WIDTH * 4;
    r->capture.scale = r->scale; r->capture.raw_ring = &r->ring; r->capture.have_mode = 1;
    r->capture.framebuffer = calloc(WIDTH * HEIGHT, 4); assert(r->capture.framebuffer);
    r->hash = UINT64_C(14695981039346656037);
}

static void mutate(replay_t *r, int step) {
    if (r->mode == 0) return;
    struct evdi_rect rect = {.x1=100, .x2=164, .y1=100, .y2=164};
    if (r->mode == 2) rect = (struct evdi_rect){.x1=0, .y1=0, .x2=WIDTH, .y2=HEIGHT};
    for (int y = rect.y1; y < rect.y2; y++)
        memset(r->capture.framebuffer + y * WIDTH * 4 + rect.x1 * 4, step, (rect.x2 - rect.x1) * 4);
    mark_damage(&r->capture, &rect, 1);
    publish_frame(&r->capture);
}

static void drain(replay_t *r) {
    char message[RAW_MESSAGE_BYTES];
    while (recv(r->peer, message, sizeof(message), MSG_DONTWAIT) > 0) {}
}

static void hash_slot(replay_t *r, unsigned slot) {
    unsigned char *pixels = r->ring.memory + RAW_CONTROL_BYTES + slot * r->ring.slot_bytes;
    size_t size = (size_t)r->ring.width * r->ring.height * 3 / 2;
    for (size_t i = 0; i < size; i++) r->hash = (r->hash ^ pixels[i]) * UINT64_C(1099511628211);
}

static int compare(const void *a, const void *b) {
    uint64_t x = *(const uint64_t *)a, y = *(const uint64_t *)b;
    return (x > y) - (x < y);
}

static void measure(replay_t *r) {
    uint64_t latency[SAMPLES], cpu = 0;
    for (unsigned slot = 0; slot < RAW_SLOTS; slot++) {
        r->capture.raw_sent_us = 0;
        assert(publish_shared_capture(&r->capture));
    }
    drain(r);
    for (unsigned step = 0; step < SAMPLES; step++) {
        unsigned slot = step % RAW_SLOTS;
        atomic_store_explicit((_Atomic uint32_t *)(r->ring.memory + slot * RAW_SLOT_CONTROL_BYTES),
                              RAW_FREE, memory_order_release);
        uint64_t cpu_start = now_ns(CLOCK_PROCESS_CPUTIME_ID), start = now_ns(CLOCK_MONOTONIC);
        mutate(r, step);
        r->capture.raw_sent_us = 0;
        assert(publish_shared_capture(&r->capture));
        latency[step] = now_ns(CLOCK_MONOTONIC) - start;
        cpu += now_ns(CLOCK_PROCESS_CPUTIME_ID) - cpu_start;
        hash_slot(r, slot); drain(r);
    }
    qsort(latency, SAMPLES, sizeof(*latency), compare);
    printf("{\"scale\":%d,\"mode\":%d,\"samples\":%d,\"p50_us\":%.3f,\"p95_us\":%.3f,"
           "\"cpu_us\":%.3f,\"checksum\":\"%016" PRIx64 "\"}\n", r->scale, r->mode, SAMPLES,
           latency[SAMPLES / 2] / 1000.0, latency[SAMPLES * 95 / 100] / 1000.0,
           cpu / (1000.0 * SAMPLES), r->hash);
}

int main(int argc, char **argv) {
    assert(argc == 3);
    replay_t r = {.scale=atoi(argv[1]), .mode=atoi(argv[2])};
    assert(r.scale >= 1 && r.scale <= 4);
    assert(r.mode >= 0 && r.mode <= 2);
    initialize(&r); measure(&r);
    raw_ring_close(&r.ring); close(r.peer);
    conv_pool_destroy(&r.pool); free(r.capture.framebuffer);
    pthread_cond_destroy(&r.frames.ready); pthread_mutex_destroy(&r.frames.mutex);
    return 0;
}
