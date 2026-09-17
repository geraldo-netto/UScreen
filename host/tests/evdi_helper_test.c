/* Exercise the production helper without an EVDI module or libevdi.
 * Unused hardware paths are removed by --gc-sections. */
#define _GNU_SOURCE
#include <pthread.h>
#include <poll.h>
#include <unistd.h>
#include <stdatomic.h>
#include <stdlib.h>
static int mock_pthread_create(pthread_t *, const pthread_attr_t *, void *(*)(void *), void *);
static long mock_sysconf(int);
static int mock_poll(struct pollfd *, nfds_t, int);
static int mock_nanosleep(const struct timespec *, struct timespec *);
static void *mock_malloc(size_t);
static int mock_posix_memalign(void **, size_t, size_t);
static int mock_clock_gettime(clockid_t, struct timespec *);
#define pthread_create mock_pthread_create
#define sysconf mock_sysconf
#define poll mock_poll
#define nanosleep mock_nanosleep
#define malloc mock_malloc
#define posix_memalign mock_posix_memalign
#define clock_gettime mock_clock_gettime
#define main evdi_helper_main
#include "../evdi/evdi_helper.c"
#undef main
#undef pthread_create
#undef sysconf
#undef poll
#undef nanosleep
#undef malloc
#undef posix_memalign
#undef clock_gettime
#include <assert.h>
#include <sys/wait.h>

static int fail_worker = 0;
static int stall_once = 0;
static int add_result = 0;
static int allocation_countdown = 0;
static long long mock_monotonic_ms = -1;
static int mock_clock_gettime(clockid_t clock, struct timespec *value) {
    if (clock == CLOCK_MONOTONIC && mock_monotonic_ms >= 0) {
        value->tv_sec = mock_monotonic_ms / 1000;
        value->tv_nsec = (mock_monotonic_ms % 1000) * 1000000;
        return 0;
    }
    return clock_gettime(clock, value);
}
static void *mock_malloc(size_t size) {
    if (allocation_countdown > 0 && --allocation_countdown == 0) return NULL;
    return malloc(size);
}
static int mock_posix_memalign(void **pointer, size_t alignment, size_t size) {
    /* Exercise the ordinary-allocation fallback, including its failure. */
    if (allocation_countdown > 0) return ENOMEM;
    return posix_memalign(pointer, alignment, size);
}
static atomic_int pause_writer = 0;
static atomic_int writer_paused = 0;
static int mock_nanosleep(const struct timespec *request, struct timespec *remainder) {
    if (pause_writer && g_writer_busy) {
        writer_paused = 1;
        while (pause_writer) usleep(1000);
        return 0;
    }
    return nanosleep(request, remainder);
}
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

static char mock_card_root[4096];
struct evdi_device_context { int fd; };
evdi_handle evdi_open(int card) {
    char path[4096];
    snprintf(path, sizeof(path), "%s/card%d", mock_card_root, card);
    int fd = open(path, O_RDWR);
    if (fd < 0) return EVDI_INVALID_HANDLE;
    evdi_handle handle = malloc(sizeof(*handle));
    assert(handle);
    handle->fd = fd;
    return handle;
}
void evdi_close(evdi_handle handle) { close(handle->fd); free(handle); }
evdi_selectable evdi_get_event_ready(evdi_handle handle) { return handle->fd; }

void evdi_handle_events(evdi_handle handle, struct evdi_event_context *context) {
    (void)handle; (void)context;
    assert(0 && "failed event channel must not dispatch EVDI events");
}
bool evdi_request_update(evdi_handle handle, int buffer) {
    (void)handle; (void)buffer;
    assert(0 && "failed event channel must not request a capture");
    return false;
}
void evdi_grab_pixels(evdi_handle handle, struct evdi_rect *rects, int *count) {
    (void)handle; (void)rects; (void)count;
    assert(0 && "failed event channel must not grab pixels");
}

static void make_test_card(const char *root, int card) {
    char path[4096];
    snprintf(path, sizeof(path), "%s/evdi.%d", root, card); assert(mkdir(path, 0700) == 0);
    snprintf(path, sizeof(path), "%s/evdi.%d/drm", root, card); assert(mkdir(path, 0700) == 0);
    snprintf(path, sizeof(path), "%s/evdi.%d/drm/card%d", root, card, card); assert(mkdir(path, 0700) == 0);
    snprintf(path, sizeof(path), "%s/card%d", root, card);
    int fd = open(path, O_CREAT | O_RDWR, 0600); assert(fd >= 0); close(fd);
}

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

static void *cancel_fifo_write(void *arg) {
    usleep(50000);
    if ((intptr_t)arg == 1) g_mode_generation++;
    else handle_signal(SIGTERM);
    return NULL;
}

static void assert_stalled_write_exits(int reader, int cancel) {
    close(reader);
    pthread_t cancellation;
    if (cancel) assert(pthread_create(&cancellation, NULL, cancel_fifo_write, (void *)(intptr_t)cancel) == 0);
    size_t size = 2u << 20;
    unsigned char *frame = calloc(1, size);
    assert(frame);
    long long start = now_ms();
    size_t remaining = write_fifo_frame(frame, size);
    assert(remaining > 0 && g_capture_fifo_fd == -1);
    assert(now_ms() - start < (cancel ? 600 : 1400));
    if (cancel) pthread_join(cancellation, NULL);
    free(frame);
}

static void test_stalled_fifo(int cancel) {
    char root[] = "/tmp/uscreen-fifo-test-XXXXXX";
    assert(mkdtemp(root));
    char path[4096];
    snprintf(path, sizeof(path), "%s/capture.fifo", root);
    assert(mkfifo(path, 0600) == 0);
    int reader = open(path, O_RDONLY | O_NONBLOCK);
    assert(reader >= 0);
    g_fifo_path = path;
    g_capture_fifo_fd = try_open_fifo();
    assert(g_capture_fifo_fd >= 0);
    pid_t child = fork();
    assert(child >= 0);
    if (child == 0) {
        assert_stalled_write_exits(reader, cancel);
        _exit(0);
    }
    int status = 0;
    int finished = 0;
    long long deadline = now_ms() + 1600;
    while (now_ms() < deadline) {
        if (waitpid(child, &status, WNOHANG) == child) { finished = 1; break; }
        usleep(10000);
    }
    if (!finished) { kill(child, SIGKILL); waitpid(child, &status, 0); }
    close(reader);
    close(g_capture_fifo_fd);
    g_capture_fifo_fd = -1;
    unlink(path);
    rmdir(root);
    assert(finished && WIFEXITED(status) && WEXITSTATUS(status) == 0 && "T113: stalled FIFO ignores deadline/cancellation");
}

static void test_helper_options(void) {
    char *args[] = {"helper", "--unknown", "--edid", "sample.edid", "--capture-fifo", "/tmp/sample.fifo",
        "--fps", "999", "--scale", "9", "--card", "4", "--edid"};
    helper_options_t options = parse_helper_options((int)(sizeof(args) / sizeof(args[0])), args);
    assert(strcmp(options.edid_path, "sample.edid") == 0);
    assert(strcmp(options.fifo_path, "/tmp/sample.fifo") == 0);
    assert(g_fps == 60);
    assert(g_scale == 4);
    assert(g_pin_card == 4);
    char *low[] = {"helper", "--scale", "0", "--fps", "0", "--card", "-1"};
    options = parse_helper_options((int)(sizeof(low) / sizeof(low[0])), low);
    assert(options.edid_path == NULL);
    assert(options.fifo_path == NULL);
    assert(g_scale == 1);
    assert(g_fps == 60);
    assert(g_pin_card == -1);
}

static void assert_external_card_preserved(const char *root) {
    int index = -1;
    char path[4096];
    snprintf(path, sizeof(path), "%s/evdi.0/drm/card0/card0-DVI-I-1", root);
    assert(mkdir(path, 0700) == 0);
    strncat(path, "/status", sizeof(path) - strlen(path) - 1);
    FILE *status = fopen(path, "w"); assert(status);
    fputs("connected\n", status); fclose(status);
    evdi_handle external = open_available_device_in(root, -1, &index);
    assert(external && index == 2 && "T108: do not steal another EVDI application's output");
    evdi_close(external);
    assert(!open_available_device_in(root, 0, &index));
    unlink(path);
    snprintf(path, sizeof(path), "%s/evdi.0/drm/card0/card0-DVI-I-1", root); rmdir(path);
}

static void test_t108(void) {
    char root[] = "/tmp/uscreen-card-lease-test-XXXXXX";
    assert(mkdtemp(root));
    snprintf(mock_card_root, sizeof(mock_card_root), "%s", root);
    make_test_card(root, 0); make_test_card(root, 2);
    int index = -1;
    evdi_handle first = open_available_device_in(root, -1, &index);
    assert(first && index == 0);
    evdi_handle second = open_available_device_in(root, -1, &index);
    assert(second && index == 2 && "T108: another helper already owns the lowest card");
    assert(!open_available_device_in(root, 0, &index) && "T108: pinned busy card must not fall back");
    make_test_card(root, 4);
    evdi_handle added = open_available_device_in(root, -1, &index);
    assert(added && index == 4 && "T108: discover newly added free card after busy cards");
    evdi_close(first); evdi_close(second); evdi_close(added);
    evdi_handle reclaimed = open_available_device_in(root, -1, &index);
    assert(reclaimed && index == 0);
    evdi_close(reclaimed);
    assert_external_card_preserved(root);
    char path[4096];
    for (int card = 0; card <= 4; card += 2) {
        snprintf(path, sizeof(path), "%s/card%d", root, card); unlink(path);
        snprintf(path, sizeof(path), "%s/evdi.%d/drm/card%d", root, card, card); rmdir(path);
        snprintf(path, sizeof(path), "%s/evdi.%d/drm", root, card); rmdir(path);
        snprintf(path, sizeof(path), "%s/evdi.%d", root, card); rmdir(path);
    }
    assert(rmdir(root) == 0);
}

static void assert_small_scaled_modes(int scale) {
    for (int small = 1; small < 2 * scale; small++) {
        for (int axis = 0; axis < 2; axis++) {
            g_running = 1;
            struct evdi_mode mode = {axis ? 2 * scale : small, axis ? small : 2 * scale, 60, 32, 0x34325258};
            on_mode_changed(mode, NULL);
            assert(!g_have_mode && !g_buffers_ready && !g_running && "T082: undersized mode reaches conversion");
        }
    }
}

static void test_t082(void) {
    pthread_cond_init(&g_frame_ready, NULL);
    for (int scale = 1; scale <= 4; scale++) {
        g_scale = scale;
        assert_small_scaled_modes(scale);
        g_running = 1;
        struct evdi_mode mode = {2 * scale, 2 * scale, 60, 32, 0x34325258};
        on_mode_changed(mode, NULL);
        assert(g_out_w == 2 && g_out_h == 2 && g_have_mode);
        test_conversion(2 * scale, 2 * scale, scale);
        test_conversion(2 * scale + 1, 2 * scale + 1, scale);
    }
    free(g_framebuffer);
    free(g_fill); free(g_latest); free(g_write);
    free(g_dirty_fill); free(g_dirty_latest); free(g_dirty_write);
    pthread_cond_destroy(&g_frame_ready);
}

static void test_t113(void) {
    test_stalled_fifo(0);
    test_stalled_fifo(1);
    test_stalled_fifo(2);
}

static void test_t083(void) {
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
}

static void test_t081(void) {
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
}

static void test_t012(void) {
    test_conversion(6, 4, 1);
    test_conversion(7, 4, 1);
    test_conversion(6, 5, 1);
    test_conversion(7, 5, 1);
    test_conversion(13, 9, 2);
}

static void test_t013(void) {
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
}

static void stop_test_pool(void) {
    pthread_mutex_lock(&g_pool_mtx);
    g_pool_shutdown = 1;
    pthread_cond_broadcast(&g_pool_go);
    pthread_mutex_unlock(&g_pool_mtx);
    for (int i = 1; i < g_nthreads; i++) pthread_join(g_pool[i], NULL);
}

static void test_t047(void) {
    fail_worker = 2;
    conv_pool_init();
    assert(g_nthreads == 2 && "T047: count only successfully created workers");
    test_conversion(8, 8, 1);
    stop_test_pool();
}

/* Dispatch the previous valid jobs at a chosen epoch and wait until each
 * worker has observed it before exercising the next production dispatch. */
static void seed_test_pool_epoch(unsigned int epoch) {
    pthread_mutex_lock(&g_pool_mtx);
    g_pool_gen = epoch;
    g_pool_active = g_nthreads - 1;
    pthread_cond_broadcast(&g_pool_go);
    while (g_pool_active > 0) pthread_cond_wait(&g_pool_done, &g_pool_mtx);
    pthread_mutex_unlock(&g_pool_mtx);
}

static void test_t272(void) {
    int pipefd[2];
    assert(pipe(pipefd) == 0);
    assert(close(pipefd[1]) == 0);
    struct evdi_device_context handle = {.fd = pipefd[0]};
    g_have_mode = 0;
    g_running = 1;
    alarm(2); /* T272: a persistent hangup must not spin forever. */
    run_event_loop(&handle);
    alarm(0);
    assert(close(pipefd[0]) == 0);
    alarm(2); /* T272: POLLNVAL must retire the loop too. */
    run_event_loop(&handle);
    alarm(0);
    assert(pipe(pipefd) == 0);
    assert(close(pipefd[0]) == 0);
    handle.fd = pipefd[1];
    alarm(2); /* T272: a pipe writer without readers reports POLLERR. */
    run_event_loop(&handle);
    alarm(0);
    assert(close(pipefd[1]) == 0);
}

static void test_t274(void) {
    alarm(5);
    int pipefd[2];
    pthread_t writer = start_test_writer(pipefd);
    read_test_frame(pipefd[0], 8, 8);
    pause_writer = 1;
    while (!writer_paused) {
        publish_frame();
        usleep(1000);
    }
    struct evdi_mode smaller = {4, 4, 60, 32, 0x34325258};
    long long started = now_ms();
    on_mode_changed(smaller, NULL);
    assert(now_ms() - started < 2500 && "T274: mode retirement must remain bounded");
    pause_writer = 0;
    while (g_writer_busy) usleep(1000);
    unsigned char bytes[96];
    assert(read(pipefd[0], bytes, sizeof(bytes)) == 0 &&
           "T274: retired writer must close without sending stale frame bytes");
    stop_test_writer(writer, pipefd);
    alarm(0);
}

static void check_allocation_failure(int allocation, int changed_mode) {
    pthread_cond_init(&g_frame_ready, NULL);
    struct evdi_mode mode = {8, 8, 60, 32, 0x34325258};
    if (changed_mode) {
        on_mode_changed(mode, NULL);
        assert(g_have_mode && g_buffers_ready && g_buffer_registered);
        mode.width = 10;
    }
    allocation_countdown = allocation;
    on_mode_changed(mode, NULL);
    assert(!g_running && "T279: failed allocation must allow helper-exit recovery");
    assert(!g_have_mode && !g_buffers_ready && !g_buffer_registered);
    free(g_framebuffer);
    free(g_fill); free(g_latest); free(g_write);
    free(g_dirty_fill); free(g_dirty_latest); free(g_dirty_write);
    pthread_cond_destroy(&g_frame_ready);
}

static void test_t279(void) {
    for (int changed_mode = 0; changed_mode <= 1; changed_mode++) {
        for (int allocation = 1; allocation <= 7; allocation++) {
            pid_t child = fork();
            assert(child >= 0);
            if (child == 0) {
                check_allocation_failure(allocation, changed_mode);
                _exit(0);
            }
            int status;
            assert(waitpid(child, &status, 0) == child);
            assert(WIFEXITED(status) && WEXITSTATUS(status) == 0 &&
                   "T279: capture continued after mode allocation failure");
        }
    }
}

static void test_t254(void) {
    alarm(5); /* A missed dispatch must fail instead of hanging the suite. */
    unsigned char source[8 * 8 * 4] = {0};
    unsigned char destination[8 * 8 * 3 / 2];
    g_mode_w = g_mode_h = g_out_w = g_out_h = 8;
    g_mode_stride = 8 * 4;
    g_scale = 1;
    conv_pool_init();
    assert(g_nthreads > 1);
    bgra_to_nv12(source, destination, NULL);
    seed_test_pool_epoch(INT_MAX);
    bgra_to_nv12(source, destination, NULL); /* UBSan catches signed overflow. */
    seed_test_pool_epoch(UINT_MAX);
    memset(destination, 0, sizeof(destination));
    bgra_to_nv12(source, destination, NULL);
    assert(g_pool_gen == 0 && "T254: generation wraps to zero");
    for (size_t i = 0; i < sizeof(destination); i++)
        assert(destination[i] == (i < 64 ? 16 : 128) && "T254: all workers finish the wrapped frame");
    stop_test_pool();
    alarm(0);
}

static void test_t048(void) {
    pthread_cond_init(&g_frame_ready, NULL);
    struct evdi_mode mode = {8, 8, 60, 16, 0x36314752};
    on_mode_changed(mode, NULL);
    assert(!g_have_mode && !g_buffers_ready && "T048: reject non-BGRA data before conversion");
    mode.bits_per_pixel = 32;
    mode.pixel_format = 0x34324258; /* XBGR8888 is not XRGB8888. */
    on_mode_changed(mode, NULL);
    assert(!g_have_mode && !g_buffers_ready);
}

static void test_t049(void) {
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
}

static void test_t050(void) {
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
}

static void test_t051(void) {
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
}

static void test_t052(void) {
    add_result = 0;
    assert(!request_evdi_device() && "T052: libevdi reports failure as zero bytes written");
    add_result = -1;
    assert(!request_evdi_device());
    add_result = 1;
    assert(request_evdi_device());
}

static void test_t290(void) {
    const long long uptimes[] = {
        1234, (long long)INT_MAX - 1, (long long)INT_MAX + 17,
        (long long)INT_MAX + 18, (long long)UINT_MAX + 18,
        (1LL << 40) + INT_MAX + 1000
    };
    for (size_t i = 0; i < sizeof(uptimes) / sizeof(uptimes[0]); i++) {
        mock_monotonic_ms = uptimes[i];
        g_have_mode = 1;
        g_update_pending = 0;
        g_last_request_ms = 0; /* on_update_ready requests an immediate capture */
        assert(capture_poll_timeout(16) == 0 && "T290: overdue pipeline capture gained a poll delay");
        g_last_request_ms = mock_monotonic_ms;
        assert(capture_poll_timeout(16) == 4);
        g_last_request_ms = mock_monotonic_ms - 15;
        assert(capture_poll_timeout(16) == 1);
        g_last_request_ms = mock_monotonic_ms - 16;
        assert(capture_poll_timeout(16) == 0);
        g_update_pending = 1;
        g_last_request_ms = mock_monotonic_ms - 100;
        assert(capture_poll_timeout(16) == 150);
        g_last_request_ms = mock_monotonic_ms - 251;
        assert(capture_poll_timeout(16) == 0);
        g_have_mode = 0;
        assert(capture_poll_timeout(16) == 100);
    }
    mock_monotonic_ms = -1;
}

int main(int argc, char **argv) {
    assert(argc == 2);
    static const struct { const char *id; void (*run)(void); } cases[] = {
        {"T290", test_t290},
        {"T279", test_t279},
        {"T274", test_t274},
        {"T272", test_t272},
        {"T254", test_t254},
        {"T170", test_helper_options},
        {"T108", test_t108},
        {"T082", test_t082},
        {"T113", test_t113},
        {"T083", test_t083},
        {"T081", test_t081},
        {"T012", test_t012},
        {"T013", test_t013},
        {"T047", test_t047},
        {"T048", test_t048},
        {"T049", test_t049},
        {"T050", test_t050},
        {"T051", test_t051},
        {"T052", test_t052},
    };
    for (size_t i = 0; i < sizeof(cases) / sizeof(cases[0]); i++) {
        if (strcmp(argv[1], cases[i].id) == 0) {
            cases[i].run();
            return 0;
        }
    }
    assert(0 && "unknown regression case");
    return 0;
}
