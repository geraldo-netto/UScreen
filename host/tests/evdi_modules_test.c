/* T380: link public module interfaces as separate translation units. */
#define _GNU_SOURCE
#include "conversion.h"
#include "frame_exchange.h"
#include "fifo_writer.h"
#include "capture.h"
#include "writer.h"
#include <assert.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <limits.h>

static void conversion_case(conv_pool_t *pool, int scale) {
    unsigned char source[40 * 8], actual[96], reference[96];
    for (size_t i = 0; i < sizeof(source); i++) source[i] = (unsigned char)(i * 37);
    memset(actual, 0xDA, sizeof(actual));
    memset(reference, 0xDA, sizeof(reference));
    int width = 8 / scale;
    unsigned char dirty = 0x05;
    conv_job_t job = {source, actual, actual + width * width,
        8, 8, 40, width, width, scale, 0, width / 2, &dirty};
    conv_pool_convert(pool, &job);
    conv_pool_t serial = CONV_POOL_INITIALIZER;
    job.ydst = reference;
    job.uvdst = reference + width * width;
    conv_pool_convert(&serial, &job);
    assert(memcmp(actual, reference, sizeof(actual)) == 0);
    conv_pool_destroy(&serial);
}

static void *convert_independently(void *context) {
    conv_pool_t *pool = context;
    for (int i = 0; i < 20; i++) {
        conversion_case(pool, 1);
        conversion_case(pool, 2);
        conversion_case(pool, 4);
    }
    return NULL;
}

static void independent_pools(void) {
    conv_pool_t first = CONV_POOL_INITIALIZER, second = CONV_POOL_INITIALIZER;
    conv_pool_start(&first, 4);
    conv_pool_start(&second, 2);
    pthread_t a, b;
    assert(pthread_create(&a, NULL, convert_independently, &first) == 0);
    assert(pthread_create(&b, NULL, convert_independently, &second) == 0);
    pthread_join(a, NULL);
    pthread_join(b, NULL);
    conv_pool_destroy(&first);
    conv_pool_destroy(&second);
}

static void destroy_exchange(frame_exchange_t *frames) {
    frame_exchange_free(frames);
    pthread_cond_destroy(&frames->ready);
    pthread_mutex_destroy(&frames->mutex);
}

static void leased_frame_history(void) {
    frame_exchange_t frames = FRAME_EXCHANGE_INITIALIZER;
    frame_exchange_init(&frames);
    frame_exchange_resize(&frames, 8, 8);
    assert(frame_exchange_allocated(&frames));
    frames.buffers_ready = 1;
    atomic_int running = 1;
    frame_cursor_t cursor = {.generation = UINT_MAX};
    frame_lease_t lease;
    memset(frames.fill, 0x71, frames.size);
    unsigned char *published = frames.fill, *history = frames.dirty_fill;
    frame_exchange_publish(&frames, 123);
    assert(frame_exchange_claim(&frames, &cursor, &running, 1000, &lease) == 1);
    assert(lease.data == published && frames.dirty_write == history);
    assert(lease.size == 96 && lease.fresh && lease.grabbed_us == 123);
    assert(frames.writer_busy && history[0] == 0);
    frame_exchange_damage(&frames, 2, 4, 1);
    assert(history[0] == 2 && (frames.dirty_fill[0] & 2));
    memset(frames.fill, 0x22, frames.size);
    frame_exchange_publish(&frames, 456);
    assert(lease.data[0] == 0x71 && "T380: publication overwrote writer lease");
    frame_exchange_release(&frames);
    assert(frame_exchange_claim(&frames, &cursor, &running, 1000, &lease) == 1);
    assert(lease.data[0] == 0x22 && lease.grabbed_us == 456);
    /* Retirement fails closed while an outstanding lease may read old data. */
    const unsigned char *held = lease.data;
    assert(!frame_exchange_retire(&frames));
    assert(held[0] == 0x22 && !frames.buffers_ready);
    frame_exchange_release(&frames);
    assert(frame_exchange_retire(&frames));
    frame_exchange_resize(&frames, 4, 4);
    frames.buffers_ready = 1;
    assert(frame_exchange_claim(&frames, &cursor, &running, 1000, &lease) == 0);
    running = 0;
    assert(frame_exchange_claim(&frames, &cursor, &running, 1000, &lease) == -1);
    destroy_exchange(&frames);
}

static void independent_fifo_epochs(void) {
    atomic_int running = 1;
    atomic_uint generation = 7;
    fifo_writer_t a = FIFO_WRITER_INITIALIZER(&running, &generation);
    fifo_writer_t b = FIFO_WRITER_INITIALIZER(&running, &generation);
    int first[2], second[2];
    assert(pipe2(first, O_NONBLOCK) == 0 && pipe2(second, O_NONBLOCK) == 0);
    a.fd = first[1]; b.fd = second[1];
    const unsigned char data[] = {1, 2, 3};
    assert(fifo_writer_write(&a, data, sizeof(data), 7) == 0);
    generation++;
    assert(fifo_writer_write(&b, data, sizeof(data), 7) == sizeof(data));
    unsigned char readback[3];
    assert(read(first[0], readback, 3) == 3 && memcmp(data, readback, 3) == 0);
    assert(read(second[0], readback, 3) == -1);
    assert(fifo_writer_write(&b, data, sizeof(data), 8) == 0);
    fifo_writer_close(&a); fifo_writer_close(&b);
    assert(a.fd == -1 && b.fd == -1);
    close(first[0]); close(second[0]);
}

/* A fake EVDI source dispatches one mode into the supplied capture context. */
struct evdi_device_context { int fd; capture_context_t *capture; };
evdi_selectable evdi_get_event_ready(evdi_handle handle) { return handle->fd; }
void evdi_register_buffer(evdi_handle handle, struct evdi_buffer buffer) {
    assert(buffer.buffer == handle->capture->framebuffer);
    assert(buffer.width == 8 && buffer.height == 8);
}
void evdi_unregister_buffer(evdi_handle handle, int id) { (void)handle; assert(id == 0); }
bool evdi_request_update(evdi_handle handle, int id) { (void)handle; (void)id; return false; }
void evdi_grab_pixels(evdi_handle handle, struct evdi_rect *rects, int *count) {
    (void)handle; (void)rects; *count = 0;
}
void evdi_handle_events(evdi_handle handle, struct evdi_event_context *events) {
    assert(events->user_data == handle->capture);
    struct evdi_mode mode = {.width = 8, .height = 8, .refresh_rate = 60,
        .bits_per_pixel = 32, .pixel_format = 0x34325258};
    events->mode_changed_handler(mode, events->user_data);
    *handle->capture->running = 0;
}

static void callback_context(void) {
    frame_exchange_t frames = FRAME_EXCHANGE_INITIALIZER;
    frame_exchange_init(&frames);
    conv_pool_t pool = CONV_POOL_INITIALIZER;
    atomic_int running = 1;
    capture_context_t capture = CAPTURE_INITIALIZER(&frames, &pool, &running);
    int channel[2];
    assert(pipe(channel) == 0 && write(channel[1], "x", 1) == 1);
    struct evdi_device_context handle = {channel[0], &capture};
    capture.handle = &handle;
    assert(capture_run(&capture, &handle) == 0);
    assert(capture.have_mode && capture.buffer_registered && frames.latest_valid);
    assert(frames.width == 8 && frames.height == 8);
    free(capture.framebuffer);
    conv_pool_destroy(&pool);
    destroy_exchange(&frames);
    close(channel[0]); close(channel[1]);
}

int main(void) {
    alarm(15);
    independent_pools();
    leased_frame_history();
    independent_fifo_epochs();
    callback_context();
    return 0;
}
