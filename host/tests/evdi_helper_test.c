/* Exercise the production helper without an EVDI module or libevdi.
 * Unused hardware paths are removed by --gc-sections. */
#define _GNU_SOURCE
#include <pthread.h>
#include <poll.h>
#include <unistd.h>
#include <stdatomic.h>
static int mock_pthread_create(pthread_t *, const pthread_attr_t *, void *(*)(void *), void *);
static long mock_sysconf(int);
static int mock_poll(struct pollfd *, nfds_t, int);
#define pthread_create mock_pthread_create
#define sysconf mock_sysconf
#define poll mock_poll
#define main evdi_helper_main
#include "../evdi/evdi_helper.c"
#undef main
#undef pthread_create
#undef sysconf
#undef poll
#include <assert.h>

static int fail_worker = 0;
static int stall_once = 0;
static int add_result = 0;
static int mock_pthread_create(pthread_t *thread, const pthread_attr_t *attrs,
                               void *(*start)(void *), void *arg) {
    if (fail_worker && (intptr_t)arg == fail_worker) return EAGAIN;
    return pthread_create(thread, attrs, start, arg);
}
static long mock_sysconf(int name) {
    return name == _SC_NPROCESSORS_ONLN ? 8 : sysconf(name);
}
static int mock_poll(struct pollfd *fds, nfds_t n, int timeout) {
    if (stall_once) { stall_once = 0; return 0; }
    return poll(fds, n, timeout);
}
int evdi_add_device(void) { return add_result; }

static atomic_int sample_started = 0;
static atomic_int sample_finished = 0;
static void *sample_latency(void *arg) {
    (void)arg;
    atomic_store(&sample_started, 1);
    record_latency(now_us() - 1000);
    atomic_store(&sample_finished, 1);
    return NULL;
}

void evdi_register_buffer(evdi_handle handle, struct evdi_buffer buffer) {
    (void)handle;
    assert(buffer.buffer != NULL);
    assert(buffer.stride >= buffer.width * 4);
}

void evdi_unregister_buffer(evdi_handle handle, int id) {
    (void)handle;
    assert(id == 0);
}

/* TODO T012: odd source dimensions must be cropped to the packed NV12 size.
 * Guard bytes detect writes past the allocation; plane contents detect a
 * wrong destination stride, even when the write stays inside the buffer. */
static void test_conversion(int width, int height, int scale) {
    g_mode_w = width;
    g_mode_h = height;
    g_mode_stride = (width * 4 + 63) & ~63;
    g_scale = scale;
    g_out_w = (width / scale) & ~1;
    g_out_h = (height / scale) & ~1;
    const size_t pixels = (size_t)g_out_w * g_out_h;
    const size_t size = pixels * 3 / 2;
    unsigned char *source = calloc((size_t)g_mode_stride, height);
    unsigned char *dest = malloc(size + 64);
    assert(source && dest);
    memset(dest, 0xa5, size + 64);
    bgra_to_nv12(source, dest, NULL);
    for (size_t i = size; i < size + 64; i++)
        assert(dest[i] == 0xa5 && "T012: conversion wrote beyond packed NV12 buffer");
    for (size_t i = 0; i < pixels; i++)
        assert(dest[i] == 16 && "T012: wrong luma row stride");
    for (size_t i = pixels; i < size; i++)
        assert(dest[i] == 128 && "T012: wrong chroma row stride");
    free(source);
    free(dest);
}

static pthread_t start_test_writer(int pipefd[2]) {
    pthread_condattr_t attr;
    pthread_condattr_init(&attr);
    pthread_condattr_setclock(&attr, CLOCK_MONOTONIC);
    pthread_cond_init(&g_frame_ready, &attr);
    pthread_condattr_destroy(&attr);
    assert(pipe2(pipefd, O_NONBLOCK) == 0);
    g_capture_fifo_fd = pipefd[1];
    g_fps = 100;
    struct evdi_mode mode = {8, 8, 60, 32, 0x34325258};
    on_mode_changed(mode, NULL);
    pthread_t writer;
    assert(pthread_create(&writer, NULL, writer_thread, NULL) == 0);
    return writer;
}

static void read_test_frame(int fd, int width, int height) {
    unsigned char data[1024];
    size_t remaining = (size_t)width * height * 3 / 2;
    long long deadline = now_ms() + 1000;
    while (remaining && now_ms() < deadline) {
        ssize_t n = read(fd, data, remaining < sizeof(data) ? remaining : sizeof(data));
        if (n > 0) remaining -= (size_t)n;
        else usleep(1000);
    }
    assert(remaining == 0 && "test writer did not deliver its frame");
}

static void stop_test_writer(pthread_t writer, int pipefd[2]) {
    g_running = 0;
    pthread_mutex_lock(&g_swap_mutex);
    pthread_cond_broadcast(&g_frame_ready);
    pthread_mutex_unlock(&g_swap_mutex);
    pthread_join(writer, NULL);
    close(pipefd[0]);
    if (g_capture_fifo_fd >= 0) close(g_capture_fifo_fd);
    free(g_framebuffer);
    free(g_fill); free(g_latest); free(g_write);
    free(g_dirty_fill); free(g_dirty_latest); free(g_dirty_write);
    pthread_cond_destroy(&g_frame_ready);
}

int main(int argc, char **argv) {
    assert(argc == 2);
    if (strcmp(argv[1], "T083") == 0) {
        int pipefd[2];
        pthread_t writer = start_test_writer(pipefd);
        read_test_frame(pipefd[0], 8, 8);
        for (int i = 0; i < 50; i++) {
            struct evdi_mode mode = {8 + (i % 2) * 2, 8, 60, 32, 0x34325258};
            on_mode_changed(mode, NULL);
            publish_frame();
            read_test_frame(pipefd[0], mode.width, mode.height);
        }
        signal(SIGTERM, handle_signal);
        assert(pthread_kill(writer, SIGTERM) == 0);
        stop_test_writer(writer, pipefd);
    } else if (strcmp(argv[1], "T081") == 0) {
        int pipefd[2];
        pthread_t writer = start_test_writer(pipefd);
        read_test_frame(pipefd[0], 8, 8);
        for (int i = 0; i < 3; i++) {
            usleep(50000); /* several unchanged frames, before the idle keepalive */
            long long start = now_ms();
            struct evdi_mode mode = {10 + i * 2, 8, 60, 32, 0x34325258};
            on_mode_changed(mode, NULL);
            assert(now_ms() - start < 250 && "T081: idle writer falsely retains buffer ownership");
            read_test_frame(pipefd[0], mode.width, mode.height);
        }
        stop_test_writer(writer, pipefd);
    } else if (strcmp(argv[1], "T012") == 0) {
        test_conversion(6, 4, 1);
        test_conversion(7, 4, 1);
        test_conversion(6, 5, 1);
        test_conversion(7, 5, 1);
        test_conversion(13, 9, 2);
    } else if (strcmp(argv[1], "T013") == 0) {
        /* TODO T013: announce the packed size on initial and changed modes,
         * without emitting a false change for repeated compositor events. */
        pthread_cond_init(&g_frame_ready, NULL);
        g_scale = 2;
        struct evdi_mode mode = {13, 9, 60, 32, 0x34325258};
        on_mode_changed(mode, NULL);
        on_mode_changed(mode, NULL);
        mode.width = 20;
        mode.height = 12;
        on_mode_changed(mode, NULL);
        free(g_framebuffer);
        free(g_fill);
        free(g_latest);
        free(g_write);
        free(g_dirty_fill);
        free(g_dirty_latest);
        free(g_dirty_write);
        pthread_cond_destroy(&g_frame_ready);
    } else if (strcmp(argv[1], "T047") == 0) {
        fail_worker = 2;
        conv_pool_init();
        assert(g_nthreads == 2 && "T047: count only successfully created workers");
        test_conversion(8, 8, 1);
        pthread_mutex_lock(&g_pool_mtx);
        g_pool_shutdown = 1;
        pthread_cond_broadcast(&g_pool_go);
        pthread_mutex_unlock(&g_pool_mtx);
        for (int i = 1; i < g_nthreads; i++) pthread_join(g_pool[i], NULL);
    } else if (strcmp(argv[1], "T048") == 0) {
        pthread_cond_init(&g_frame_ready, NULL);
        struct evdi_mode mode = {8, 8, 60, 16, 0x36314752};
        on_mode_changed(mode, NULL);
        assert(!g_have_mode && !g_buffers_ready && "T048: reject non-BGRA data before conversion");
        mode.bits_per_pixel = 32;
        mode.pixel_format = 0x34324258; /* XBGR8888 is not XRGB8888. */
        on_mode_changed(mode, NULL);
        assert(!g_have_mode && !g_buffers_ready);
    } else if (strcmp(argv[1], "T049") == 0) {
        int pipefd[2];
        assert(pipe2(pipefd, O_NONBLOCK) == 0);
        g_capture_fifo_fd = pipefd[1];
        stall_once = 1;
        const unsigned char frame[] = {1, 2, 3, 4};
        assert(write_fifo_frame(frame, sizeof(frame)) == 0 && "T049: transient poll timeout lost frame");
        unsigned char received[4];
        assert(read(pipefd[0], received, sizeof(received)) == sizeof(received));
        assert(memcmp(frame, received, sizeof(frame)) == 0);
        close(pipefd[0]);
        close(pipefd[1]);
    } else if (strcmp(argv[1], "T050") == 0) {
        pthread_t thread;
        pthread_mutex_lock(&g_swap_mutex);
        assert(pthread_create(&thread, NULL, sample_latency, NULL) == 0);
        while (!atomic_load(&sample_started)) usleep(1000);
        usleep(20000);
        int finished_while_locked = atomic_load(&sample_finished);
        pthread_mutex_unlock(&g_swap_mutex);
        pthread_join(thread, NULL);
        assert(!finished_while_locked && "T050: writer bypassed the statistics lock");
        assert(g_lat_count == 1);
    } else if (strcmp(argv[1], "T051") == 0) {
        char root[] = "/tmp/uscreen-card-test-XXXXXX";
        assert(mkdtemp(root));
        const char *paths[] = {"evdi.9", "evdi.9/drm", "evdi.9/drm/card9",
                               "evdi.0", "evdi.0/drm", "evdi.0/drm/card0"};
        char path[4096];
        for (size_t i = 0; i < sizeof(paths) / sizeof(paths[0]); i++) {
            snprintf(path, sizeof(path), "%s/%s", root, paths[i]);
            assert(mkdir(path, 0700) == 0);
        }
        int card = find_evdi_device_in(root);
        for (int i = 5; i >= 0; i--) {
            snprintf(path, sizeof(path), "%s/%s", root, paths[i]);
            assert(rmdir(path) == 0);
        }
        rmdir(root);
        assert(card == 0 && "T051: use the lowest available card, including card0");
    } else if (strcmp(argv[1], "T052") == 0) {
        add_result = 0;
        assert(!request_evdi_device() && "T052: libevdi reports failure as zero bytes written");
        add_result = -1;
        assert(!request_evdi_device());
        add_result = 1;
        assert(request_evdi_device());
    } else {
        assert(0 && "unknown regression case");
    }
    return 0;
}
