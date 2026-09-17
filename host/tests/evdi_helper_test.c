/* Exercise the production helper without an EVDI module or libevdi.
 * Unused hardware paths are removed by --gc-sections. */
#define _GNU_SOURCE
#include <pthread.h>
#include <poll.h>
#include <unistd.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <dirent.h>
static DIR *mock_opendir(const char *);
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
#define opendir mock_opendir
#define main evdi_helper_main
#include "../evdi/conversion.c"
#define add_period frame_test_add_period
#include "../evdi/frame_exchange.c"
#include "../evdi/fifo_writer.c"
#include "../evdi/capture.c"
#undef add_period
#include "../evdi/writer.c"
#include "../evdi/evdi_helper.c"
#undef main
#undef pthread_create
#undef sysconf
#undef poll
#undef nanosleep
#undef malloc
#undef posix_memalign
#undef clock_gettime
#undef opendir
#include <assert.h>
#include <sys/wait.h>

static long long now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long long)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

static int fail_worker = 0;
static int stall_once = 0;
static int add_result = 0;
static int mock_add_calls;
static int mock_discovery_calls;
static int allocation_countdown = 0;
static long long mock_monotonic_ms = -1;
static int mock_grab_calls = -1;
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
    if (pause_writer && g_frames.writer_busy) {
        writer_paused = 1;
        while (pause_writer) usleep(1000);
        return 0;
    }
    return nanosleep(request, remainder);
}
static int mock_pthread_create(pthread_t *thread, const pthread_attr_t *attrs,
                               void *(*start)(void *), void *arg) {
    if (fail_worker && start == conv_worker && ((conv_worker_arg_t *)arg)->id == fail_worker) return EAGAIN;
    return pthread_create(thread, attrs, start, arg);
}
static long mock_sysconf(int name) {
    return name == _SC_NPROCESSORS_ONLN ? 8 : sysconf(name);
}
static int mock_poll(struct pollfd *fds, nfds_t n, int timeout) {
    if (stall_once) { stall_once = 0; return 0; }
    return poll(fds, n, timeout);
}
int evdi_add_device(void) { mock_add_calls++; return add_result; }

static char mock_card_root[4096];
static DIR *mock_opendir(const char *path) {
    if (strcmp(path, "/sys/devices/platform") == 0) {
        assert(mock_card_root[0] && "test must never inspect host DRM devices");
        mock_discovery_calls++;
        return opendir(mock_card_root);
    }
    return opendir(path);
}
struct evdi_device_context { int fd; };
static int mock_open_calls;
evdi_handle evdi_open(int card) {
    mock_open_calls++;
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
static int mock_disconnect_calls;
void evdi_disconnect(evdi_handle handle) { (void)handle; mock_disconnect_calls++; }
void evdi_connect(evdi_handle handle, const unsigned char *edid, unsigned int length, uint32_t limit) {
    (void)handle; (void)edid; (void)length; (void)limit;
    assert(0 && "startup validation must not connect a display");
}
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
    if (mock_grab_calls >= 0) {
        mock_grab_calls++;
        *count = 0;
        return;
    }
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
    record_latency(&g_writer, writer_now_us() - 1000);
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
    g_capture.mode_w = width;
    g_capture.mode_h = height;
    g_capture.mode_stride = (width * 4 + 63) & ~63;
    g_capture.scale = scale;
    g_frames.width = (width / scale) & ~1;
    g_frames.height = (height / scale) & ~1;
    const size_t pixels = (size_t)g_frames.width * g_frames.height;
    const size_t size = pixels * 3 / 2;
    unsigned char *source = calloc((size_t)g_capture.mode_stride, height);
    unsigned char *dest = malloc(size + 64);
    assert(source && dest);
    memset(dest, 0xa5, size + 64);
    bgra_to_nv12(&g_capture, source, dest, NULL);
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
    pthread_cond_init(&g_frames.ready, &attr);
    pthread_condattr_destroy(&attr);
    assert(pipe2(pipefd, O_NONBLOCK) == 0);
    g_fifo.fd = pipefd[1];
    g_writer.fps = g_capture.fps = 100;
    struct evdi_mode mode = {8, 8, 60, 32, 0x34325258};
    on_mode_changed(mode, &g_capture);
    pthread_t writer;
    assert(pthread_create(&writer, NULL, writer_run, &g_writer) == 0);
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
    pthread_mutex_lock(&g_frames.mutex);
    pthread_cond_broadcast(&g_frames.ready);
    pthread_mutex_unlock(&g_frames.mutex);
    pthread_join(writer, NULL);
    close(pipefd[0]);
    if (g_fifo.fd >= 0) close(g_fifo.fd);
    free(g_capture.framebuffer);
    free(g_frames.fill); free(g_frames.latest); free(g_frames.write);
    free(g_frames.dirty_fill); free(g_frames.dirty_latest); free(g_frames.dirty_write);
    pthread_cond_destroy(&g_frames.ready);
}

static void *cancel_fifo_write(void *arg) {
    usleep(50000);
    if ((intptr_t)arg == 1) g_frames.generation++;
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
    size_t remaining = fifo_writer_write(&g_fifo, frame, size, g_frames.generation);
    assert(remaining > 0 && g_fifo.fd == -1);
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
    g_fifo.path = path;
    g_fifo.fd = fifo_writer_open(&g_fifo);
    assert(g_fifo.fd >= 0);
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
    close(g_fifo.fd);
    g_fifo.fd = -1;
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
    assert(g_capture.fps == 60);
    assert(g_capture.scale == 4);
    assert(g_pin_card == 4);
    char *low[] = {"helper", "--scale", "0", "--fps", "0", "--card", "-1"};
    options = parse_helper_options((int)(sizeof(low) / sizeof(low[0])), low);
    assert(options.edid_path == NULL);
    assert(options.fifo_path == NULL);
    assert(g_capture.scale == 1);
    assert(g_capture.fps == 60);
    assert(g_pin_card == -1);
    char *preferred[] = {"helper", "--preferred-card", "7"};
    parse_helper_options((int)(sizeof(preferred) / sizeof(preferred[0])), preferred);
    assert(g_preferred_card == 7);
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
    assert(!open_session_device_in(root, 0, 2, &index) && "T330: preference cannot override a strict pin");
    external = open_session_device_in(root, -1, 0, &index);
    assert(external && index == 2 && "T330: busy preference must use a free lease");
    evdi_close(external);
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
            on_mode_changed(mode, &g_capture);
            assert(!g_capture.have_mode && !g_frames.buffers_ready && !g_running && "T082: undersized mode reaches conversion");
        }
    }
}

static void test_t082(void) {
    pthread_cond_init(&g_frames.ready, NULL);
    for (int scale = 1; scale <= 4; scale++) {
        g_capture.scale = scale;
        assert_small_scaled_modes(scale);
        g_running = 1;
        struct evdi_mode mode = {2 * scale, 2 * scale, 60, 32, 0x34325258};
        on_mode_changed(mode, &g_capture);
        assert(g_frames.width == 2 && g_frames.height == 2 && g_capture.have_mode);
        test_conversion(2 * scale, 2 * scale, scale);
        test_conversion(2 * scale + 1, 2 * scale + 1, scale);
    }
    free(g_capture.framebuffer);
    free(g_frames.fill); free(g_frames.latest); free(g_frames.write);
    free(g_frames.dirty_fill); free(g_frames.dirty_latest); free(g_frames.dirty_write);
    pthread_cond_destroy(&g_frames.ready);
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
        on_mode_changed(mode, &g_capture);
        publish_frame(&g_capture);
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
        on_mode_changed(mode, &g_capture);
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
    pthread_cond_init(&g_frames.ready, NULL);
    g_capture.scale = 2;
    struct evdi_mode mode = {13, 9, 60, 32, 0x34325258};
    on_mode_changed(mode, &g_capture);
    on_mode_changed(mode, &g_capture);
    mode.width = 20;
    mode.height = 12;
    on_mode_changed(mode, &g_capture);
    free(g_capture.framebuffer);
    free(g_frames.fill);
    free(g_frames.latest);
    free(g_frames.write);
    free(g_frames.dirty_fill);
    free(g_frames.dirty_latest);
    free(g_frames.dirty_write);
    pthread_cond_destroy(&g_frames.ready);
}

static void stop_test_pool(void) {
    pthread_mutex_lock(&g_conversion.mutex);
    g_conversion.shutdown = 1;
    pthread_cond_broadcast(&g_conversion.ready);
    pthread_mutex_unlock(&g_conversion.mutex);
    for (int i = 1; i < g_conversion.count; i++) pthread_join(g_conversion.threads[i], NULL);
}

static void test_t047(void) {
    fail_worker = 2;
    conv_pool_init();
    assert(g_conversion.count == 2 && "T047: count only successfully created workers");
    test_conversion(8, 8, 1);
    stop_test_pool();
}

/* Dispatch the previous valid jobs at a chosen epoch and wait until each
 * worker has observed it before exercising the next production dispatch. */
static void seed_test_pool_epoch(unsigned int epoch) {
    pthread_mutex_lock(&g_conversion.mutex);
    g_conversion.generation = epoch;
    g_conversion.active = g_conversion.count - 1;
    pthread_cond_broadcast(&g_conversion.ready);
    while (g_conversion.active > 0) pthread_cond_wait(&g_conversion.done, &g_conversion.mutex);
    pthread_mutex_unlock(&g_conversion.mutex);
}

static void test_t272(void) {
    int pipefd[2];
    assert(pipe(pipefd) == 0);
    assert(close(pipefd[1]) == 0);
    struct evdi_device_context handle = {.fd = pipefd[0]};
    g_capture.have_mode = 0;
    g_running = 1;
    alarm(2); /* T272: a persistent hangup must not spin forever. */
    capture_run(&g_capture, &handle);
    alarm(0);
    assert(close(pipefd[0]) == 0);
    alarm(2); /* T272: POLLNVAL must retire the loop too. */
    capture_run(&g_capture, &handle);
    alarm(0);
    assert(pipe(pipefd) == 0);
    assert(close(pipefd[0]) == 0);
    handle.fd = pipefd[1];
    alarm(2); /* T272: a pipe writer without readers reports POLLERR. */
    capture_run(&g_capture, &handle);
    alarm(0);
    assert(close(pipefd[1]) == 0);
}

static void check_t315_exit_status(int condition, int expected) {
    int pipefd[2];
    assert(pipe(pipefd) == 0);
    assert(close(pipefd[1]) == 0);
    evdi_handle handle = malloc(sizeof(*handle));
    assert(handle);
    handle->fd = pipefd[0];
    g_capture.handle = handle;
    initialize_helper_runtime();
    if (condition == 1) {
        struct evdi_mode mode = {.bits_per_pixel = 16};
        assert(!validate_frame_format(&g_capture, mode));
    } else if (condition == 2) {
        handle_signal(SIGTERM);
    }
    alarm(2);
    int status = run_capture(handle, 0);
    alarm(0);
    assert(mock_disconnect_calls == 1 && "T315: every exit must disconnect");
    assert(g_capture.handle == EVDI_INVALID_HANDLE && "T315: every exit must close the device");
    assert(status == expected && "T315: fatal errors must not report successful shutdown");
}

static void test_t315_channel(void) { check_t315_exit_status(0, 1); }
static void test_t315_mode(void) { check_t315_exit_status(1, 1); }
static void test_t315_signal(void) { check_t315_exit_status(2, 0); }

static void test_t324(void) {
    alarm(5);
    int pipefd[2];
    pthread_t writer = start_test_writer(pipefd);
    read_test_frame(pipefd[0], 8, 8);
    pause_writer = 1;
    while (!writer_paused) {
        publish_frame(&g_capture);
        usleep(1000);
    }
    /* Begin mode retirement while the writer is paused, without timing out. */
    pthread_mutex_lock(&g_frames.mutex);
    g_frames.buffers_ready = 0;
    g_frames.latest_valid = 0;
    g_frames.generation++;
    pthread_mutex_unlock(&g_frames.mutex);
    pause_writer = 0;
    while (g_frames.writer_busy) usleep(1000);
    unsigned char bytes[96];
    ssize_t received = read(pipefd[0], bytes, sizeof(bytes));
    int running = g_running;
    stop_test_writer(writer, pipefd);
    assert(running && "T324: discard a retired frame without stopping capture");
    assert(received <= 0 && "T324: writer sent a retired frame after its pacing sleep");
    alarm(0);
}

static void test_t274(void) {
    alarm(5);
    int pipefd[2];
    pthread_t writer = start_test_writer(pipefd);
    read_test_frame(pipefd[0], 8, 8);
    pause_writer = 1;
    while (!writer_paused) {
        publish_frame(&g_capture);
        usleep(1000);
    }
    struct evdi_mode smaller = {4, 4, 60, 32, 0x34325258};
    long long started = now_ms();
    on_mode_changed(smaller, &g_capture);
    assert(now_ms() - started < 2500 && "T274: mode retirement must remain bounded");
    pause_writer = 0;
    while (g_frames.writer_busy) usleep(1000);
    unsigned char bytes[96];
    assert(read(pipefd[0], bytes, sizeof(bytes)) == 0 &&
           "T274: retired writer must close without sending stale frame bytes");
    stop_test_writer(writer, pipefd);
    alarm(0);
}

static void check_allocation_failure(int allocation, int changed_mode) {
    pthread_cond_init(&g_frames.ready, NULL);
    struct evdi_mode mode = {8, 8, 60, 32, 0x34325258};
    if (changed_mode) {
        on_mode_changed(mode, &g_capture);
        assert(g_capture.have_mode && g_frames.buffers_ready && g_capture.buffer_registered);
        mode.width = 10;
    }
    allocation_countdown = allocation;
    on_mode_changed(mode, &g_capture);
    assert(!g_running && "T279: failed allocation must allow helper-exit recovery");
    assert(!g_capture.have_mode && !g_frames.buffers_ready && !g_capture.buffer_registered);
    free(g_capture.framebuffer);
    free(g_frames.fill); free(g_frames.latest); free(g_frames.write);
    free(g_frames.dirty_fill); free(g_frames.dirty_latest); free(g_frames.dirty_write);
    pthread_cond_destroy(&g_frames.ready);
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
    g_capture.mode_w = g_capture.mode_h = g_frames.width = g_frames.height = 8;
    g_capture.mode_stride = 8 * 4;
    g_capture.scale = 1;
    conv_pool_init();
    assert(g_conversion.count > 1);
    bgra_to_nv12(&g_capture, source, destination, NULL);
    seed_test_pool_epoch(INT_MAX);
    bgra_to_nv12(&g_capture, source, destination, NULL); /* UBSan catches signed overflow. */
    seed_test_pool_epoch(UINT_MAX);
    memset(destination, 0, sizeof(destination));
    bgra_to_nv12(&g_capture, source, destination, NULL);
    assert(g_conversion.generation == 0 && "T254: generation wraps to zero");
    for (size_t i = 0; i < sizeof(destination); i++)
        assert(destination[i] == (i < 64 ? 16 : 128) && "T254: all workers finish the wrapped frame");
    stop_test_pool();
    alarm(0);
}

static void test_t048(void) {
    pthread_cond_init(&g_frames.ready, NULL);
    struct evdi_mode mode = {8, 8, 60, 16, 0x36314752};
    on_mode_changed(mode, &g_capture);
    assert(!g_capture.have_mode && !g_frames.buffers_ready && "T048: reject non-BGRA data before conversion");
    mode.bits_per_pixel = 32;
    mode.pixel_format = 0x34324258; /* XBGR8888 is not XRGB8888. */
    on_mode_changed(mode, &g_capture);
    assert(!g_capture.have_mode && !g_frames.buffers_ready);
}

static void test_t049(void) {
    int pipefd[2];
    assert(pipe2(pipefd, O_NONBLOCK) == 0);
    g_fifo.fd = pipefd[1];
    stall_once = 1;
    const unsigned char frame[] = {1, 2, 3, 4};
    assert(fifo_writer_write(&g_fifo, frame, sizeof(frame), g_frames.generation) == 0 && "T049: transient poll timeout lost frame");
    unsigned char received[4];
    assert(read(pipefd[0], received, sizeof(received)) == sizeof(received));
    assert(memcmp(frame, received, sizeof(frame)) == 0);
    close(pipefd[0]);
    close(pipefd[1]);
}

static void test_t050(void) {
    pthread_t thread;
    pthread_mutex_lock(&g_frames.mutex);
    assert(pthread_create(&thread, NULL, sample_latency, NULL) == 0);
    while (!atomic_load(&sample_started)) usleep(1000);
    usleep(20000);
    int finished_while_locked = atomic_load(&sample_finished);
    pthread_mutex_unlock(&g_frames.mutex);
    pthread_join(thread, NULL);
    assert(!finished_while_locked && "T050: writer bypassed the statistics lock");
    assert(g_frames.latency_count == 1);
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

static void t293_uniform_color(int red, int green, int blue, int scale) {
    g_capture.mode_w = g_capture.mode_h = 16;
    g_capture.mode_stride = 80; /* exercise padded source rows */
    g_capture.scale = scale;
    g_frames.width = g_frames.height = (16 / scale) & ~1;
    unsigned char source[16 * 80] = {0}, dest[16 * 16 * 3 / 2];
    for (int y = 0; y < 16; y++) {
        for (int x = 0; x < 16; x++) {
            unsigned char *pixel = source + y * 80 + x * 4;
            pixel[0] = blue; pixel[1] = green; pixel[2] = red; pixel[3] = 255;
        }
    }
    bgra_to_nv12(&g_capture, source, dest, NULL);
    /* Independent equations from ITU-R BT.709-6, section 3.2–3.4.
       Input RGB is full-range 8-bit; output Y/Cb/Cr is limited-range. */
    const double luma = (0.2126 * red + 0.7152 * green + 0.0722 * blue) / 255.0;
    const int expected_y = (int)(16 + 219 * luma + 0.5);
    const int expected_u = (int)(128 + 224 * (blue / 255.0 - luma) / 1.8556 + 0.5);
    const int expected_v = (int)(128 + 224 * (red / 255.0 - luma) / 1.5748 + 0.5);
    const int pixels = g_frames.width * g_frames.height;
    for (int i = 0; i < pixels; i++) assert(abs(dest[i] - expected_y) <= 1);
    const int tolerance = (red == green && green == blue) ? 0 : 1;
    for (int i = pixels; i < pixels * 3 / 2; i += 2) {
        assert(abs(dest[i] - expected_u) <= tolerance && "T293: neutral pixels gained blue-difference chroma");
        assert(abs(dest[i + 1] - expected_v) <= tolerance);
    }
}

static void test_t293(void) {
    for (int scale = 1; scale <= 4; scale++) {
        for (int gray = 0; gray <= 255; gray++) t293_uniform_color(gray, gray, gray, scale);
        for (int color = 0; color < 8; color++)
            t293_uniform_color((color & 1) * 255, ((color >> 1) & 1) * 255,
                               ((color >> 2) & 1) * 255, scale);
    }
}

static void test_t294(void) {
    g_capture.have_mode = 1;
    for (long long elapsed = 249; elapsed <= 251; elapsed++) {
        mock_grab_calls = 0;
        g_capture.update_pending = 1;
        g_capture.last_request_ms = 1000;
        mock_monotonic_ms = 1000 + elapsed;
        long long last_fallback = mock_monotonic_ms;
        const int expired = elapsed >= 250;
        assert(capture_poll_timeout(&g_capture, 16) == (expired ? 0 : 1));
        recover_capture_if_stalled(&g_capture, mock_monotonic_ms, &last_fallback);
        assert(g_capture.update_pending == !expired && "T294: zero-timeout polling cannot retain an expired request");
        assert(mock_grab_calls == expired);
        recover_capture_if_stalled(&g_capture, mock_monotonic_ms, &last_fallback);
        assert(mock_grab_calls == expired && "T294: watchdog must not grab twice");
    }
    mock_monotonic_ms = -1;
    mock_grab_calls = -1;
}

static void test_t290(void) {
    const long long uptimes[] = {
        1234, (long long)INT_MAX - 1, (long long)INT_MAX + 17,
        (long long)INT_MAX + 18, (long long)UINT_MAX + 18,
        (1LL << 40) + INT_MAX + 1000
    };
    for (size_t i = 0; i < sizeof(uptimes) / sizeof(uptimes[0]); i++) {
        mock_monotonic_ms = uptimes[i];
        g_capture.have_mode = 1;
        g_capture.update_pending = 0;
        g_capture.last_request_ms = 0; /* on_update_ready requests an immediate capture */
        assert(capture_poll_timeout(&g_capture, 16) == 0 && "T290: overdue pipeline capture gained a poll delay");
        g_capture.last_request_ms = mock_monotonic_ms;
        assert(capture_poll_timeout(&g_capture, 16) == 4);
        g_capture.last_request_ms = mock_monotonic_ms - 15;
        assert(capture_poll_timeout(&g_capture, 16) == 1);
        g_capture.last_request_ms = mock_monotonic_ms - 16;
        assert(capture_poll_timeout(&g_capture, 16) == 0);
        g_capture.update_pending = 1;
        g_capture.last_request_ms = mock_monotonic_ms - 100;
        assert(capture_poll_timeout(&g_capture, 16) == 150);
        g_capture.last_request_ms = mock_monotonic_ms - 251;
        assert(capture_poll_timeout(&g_capture, 16) == 0);
        g_capture.have_mode = 0;
        assert(capture_poll_timeout(&g_capture, 16) == 100);
    }
    mock_monotonic_ms = -1;
}

static void t340_startup_case(int bytes) {
    char root[] = "/tmp/uscreen-t340-XXXXXX";
    assert(mkdtemp(root));
    snprintf(mock_card_root, sizeof(mock_card_root), "%s", root);
    char path[4096];
    snprintf(path, sizeof(path), "%s/edid", root);
    if (bytes >= 0) {
        int fd = open(path, O_CREAT | O_WRONLY, 0600);
        assert(fd >= 0);
        assert(ftruncate(fd, bytes) == 0);
        close(fd);
    } else if (bytes == -2) {
        assert(mkdir(path, 0700) == 0);
    }
    char *args[] = {"evdi_helper", "--edid", path};
    int status = evdi_helper_main(3, args);
    if (bytes == -2) assert(rmdir(path) == 0);
    else if (bytes >= 0) assert(unlink(path) == 0);
    assert(rmdir(root) == 0);
    assert(status == 1);
    assert(mock_open_calls == 0);
    int readable = bytes == 128;
    assert(mock_add_calls == readable && "T340: unreadable EDID requested a kernel device");
    assert(mock_discovery_calls == readable && "T340: unreadable EDID reached DRM discovery");
}

static void test_t340_missing(void) { t340_startup_case(-1); }
static void test_t340_directory(void) { t340_startup_case(-2); }
static void test_t340_empty(void) { t340_startup_case(0); }
static void test_t340_oversized(void) { t340_startup_case(32769); }
static void test_t340_readable(void) { t340_startup_case(128); }

static void t341_regular_edid(void) {
    char root[] = "/tmp/uscreen-t341-regular-XXXXXX", path[4096], link[4096];
    assert(mkdtemp(root));
    snprintf(path, sizeof(path), "%s/custom edid.bin", root);
    snprintf(link, sizeof(link), "%s/linked edid.bin", root);
    unsigned char expected[128];
    memset(expected, 0x5a, sizeof(expected));
    int fd = open(path, O_CREAT | O_WRONLY, 0600);
    assert(fd >= 0);
    assert(write(fd, expected, sizeof(expected)) == sizeof(expected));
    close(fd);
    assert(symlink("custom edid.bin", link) == 0);
    const char *inputs[] = {path, link};
    for (int i = 0; i < 2; i++) {
        long size = 0;
        unsigned char *edid = read_edid_file(inputs[i], &size);
        assert(edid && "T341: readable EDID or symlink rejected");
        assert(size == sizeof(expected));
        assert(memcmp(edid, expected, sizeof(expected)) == 0);
        free(edid);
    }
    assert(unlink(link) == 0);
    assert(unlink(path) == 0);
    assert(rmdir(root) == 0);
}

static void test_t341(void) {
    t341_regular_edid();
    char root[] = "/tmp/uscreen-t341-XXXXXX", path[4096];
    assert(mkdtemp(root));
    snprintf(path, sizeof(path), "%s/edid", root);
    assert(mkfifo(path, 0600) == 0);
    pid_t reader = fork();
    assert(reader >= 0);
    if (reader == 0) {
        alarm(2);
        long size;
        unsigned char *edid = read_edid_file(path, &size);
        assert(edid == NULL && "T341: FIFO cannot supply seekable EDID input");
        _exit(0);
    }
    int status;
    assert(waitpid(reader, &status, 0) == reader);
    assert(unlink(path) == 0);
    assert(rmdir(root) == 0);
    assert(WIFEXITED(status) && WEXITSTATUS(status) == 0 &&
           "T341: EDID reader blocked waiting for a FIFO writer");
}

static void t343_regular_destination(void) {
    char path[] = "/tmp/uscreen-t343-file-XXXXXX";
    const char original[] = "existing file contents";
    int file = mkstemp(path);
    assert(file >= 0);
    assert(write(file, original, sizeof(original)) == sizeof(original));
    g_fifo.path = path;
    int writer = fifo_writer_open(&g_fifo);
    if (writer >= 0) {
        assert(write(writer, "frame", 5) == 5);
        close(writer);
    }
    char actual[sizeof(original)];
    assert(pread(file, actual, sizeof(actual), 0) == sizeof(actual));
    close(file);
    assert(unlink(path) == 0);
    assert(memcmp(actual, original, sizeof(original)) == 0 &&
           "T343: capture overwrote an ordinary file");
    assert(writer < 0 && "T343: accepted a non-FIFO capture destination");
}

static void t343_fifo_destination(void) {
    char root[] = "/tmp/uscreen-t343-fifo-XXXXXX", path[4096], link[4096];
    assert(mkdtemp(root));
    snprintf(path, sizeof(path), "%s/capture pipe", root);
    snprintf(link, sizeof(link), "%s/linked pipe", root);
    assert(mkfifo(path, 0600) == 0);
    g_fifo.path = path;
    assert(fifo_writer_open(&g_fifo) < 0 && "T343: FIFO without reader should fail promptly");
    int reader = open(path, O_RDONLY | O_NONBLOCK);
    assert(reader >= 0);
    int writer = fifo_writer_open(&g_fifo);
    assert(writer >= 0);
    assert(write(writer, "frame", 5) == 5);
    char actual[5];
    assert(read(reader, actual, sizeof(actual)) == sizeof(actual));
    assert(memcmp(actual, "frame", sizeof(actual)) == 0);
    assert(symlink("capture pipe", link) == 0);
    g_fifo.path = link;
    assert(fifo_writer_open(&g_fifo) < 0 && "T343: symlink destination must remain rejected");
    close(writer);
    close(reader);
    assert(unlink(link) == 0);
    assert(unlink(path) == 0);
    assert(rmdir(root) == 0);
}

static void test_t343(void) {
    alarm(2);
    t343_regular_destination();
    t343_fifo_destination();
    alarm(0);
}

/* T330: production option parsing/allocation, fake DRM inodes, real flock.
   The Rust daemon fixture starts concurrent helpers and owns their lifetime. */
static int t330_command_lease(int argc, char **argv, const char *root) {
    snprintf(mock_card_root, sizeof(mock_card_root), "%s", root);
    parse_helper_options(argc, argv);
    int card = -1;
    evdi_handle handle = acquire_capture_device_in(root, &card);
    if (handle == EVDI_INVALID_HANDLE) return 1;
    printf("EVDI_CONNECTED card%d\n", card);
    fflush(stdout);
    alarm(20);
    for (;;) pause(); /* SIGTERM/SIGKILL closes the leased fake inode. */
}

/* T226: no encoder may see new bytes appended to a retired partial frame. */
static void test_t226(void) {
    char root[] = "/tmp/uscreen-t226-XXXXXX", path[4096], fresh[4096];
    assert(mkdtemp(root));
    snprintf(path, sizeof(path), "%s/frames", root);
    snprintf(fresh, sizeof(fresh), "%s/new", root);
    assert(mkfifo(path, 0600) == 0);
    int old_reader = open(path, O_RDONLY | O_NONBLOCK);
    assert(old_reader >= 0);
    g_fifo.path = path;
    assert(ensure_writer_fifo(&g_writer));
    size_t capacity = (size_t)fcntl(g_fifo.fd, F_GETPIPE_SZ);
    size_t size = capacity + 64;
    unsigned char *frame = malloc(size);
    assert(frame);
    memset(frame, 48, size);
    assert(fifo_writer_write(&g_fifo, frame, size, g_frames.generation) == 64);
    assert(!ensure_writer_fifo(&g_writer) && "T226: immediate reopen can join two frame generations");
    unsigned char *old = malloc(size);
    assert(old);
    assert(read(old_reader, old, size) == (ssize_t)capacity);
    assert(read(old_reader, old, size) == -1 && errno == EAGAIN &&
           "T226: premature EOF lets encoder exit race the reset announcement");
    assert(!ensure_writer_fifo(&g_writer) && "T226: draining the pipe does not authorize reuse");
    /* Allocate the replacement while the old inode still exists. */
    assert(mkfifo(fresh, 0600) == 0);
    assert(rename(fresh, path) == 0);
    int new_reader = open(path, O_RDONLY | O_NONBLOCK);
    assert(new_reader >= 0 && ensure_writer_fifo(&g_writer));
    memset(frame, 160, 64);
    assert(fifo_writer_write(&g_fifo, frame, 64, g_frames.generation) == 0);
    assert(read(new_reader, old, size) == 64);
    assert(memcmp(frame, old, 64) == 0);
    assert(read(old_reader, old, size) == 0 && "T226: old reader received replacement bytes");
    close(old_reader); close(new_reader); close(g_fifo.fd);
    g_fifo.fd = -1;
    free(frame); free(old); unlink(path); rmdir(root);
}

static void t226_mark(const char *root, const char *name) {
    char path[4096];
    snprintf(path, sizeof(path), "%s/%s", root, name);
    FILE *marker = fopen(path, "a");
    assert(marker);
    fputs("ready\n", marker);
    fclose(marker);
}

static void t226_requests(const char *root, int *live, int *stale) {
    char path[4096];
    snprintf(path, sizeof(path), "%s/request-live", root);
    if (!*live && access(path, F_OK) == 0) {
        *live = 1;
        /* Exercise the same recovery announcement after encoding is active. */
        retire_partial_fifo(&g_fifo);
        t226_mark(root, "live");
    }
    snprintf(path, sizeof(path), "%s/request-stale", root);
    if (!*stale && access(path, F_OK) == 0) {
        *stale = 1;
        printf("FIFO_RESET %ju %ju\n", (uintmax_t)g_fifo.retired_device,
               (uintmax_t)g_fifo.retired_inode);
        fflush(stdout);
        t226_mark(root, "stale");
    }
}

/* Real FIFO writer and stock encoder; no DRM, EVDI or input-device access. */
static int t226_command(int argc, char **argv, const char *root) {
    helper_options_t options = parse_helper_options(argc, argv);
    assert(options.fifo_path);
    g_fifo.path = options.fifo_path;
    signal(SIGTERM, handle_signal);
    signal(SIGPIPE, SIG_IGN);
    alarm(20);
    t226_mark(root, "starts");
    printf("EVDI_CONNECTED card4294967295\nSTREAM_SIZE 1024 1024\n");
    fflush(stdout);
    while (g_running && !ensure_writer_fifo(&g_writer)) {}
    size_t size = 1024 * 1024 * 3 / 2;
    unsigned char *frame = malloc(size);
    assert(frame);
    memset(frame, 48, 1024 * 1024);
    memset(frame + 1024 * 1024, 128, size - 1024 * 1024);
    size_t remaining = fifo_writer_write(&g_fifo, frame, size, g_frames.generation);
    assert(remaining > 0 && remaining < size);
    /* Reopen before the old reader drains, the original corruption trigger. */
    ensure_writer_fifo(&g_writer);
    t226_mark(root, "partial");
    memset(frame, 160, 1024 * 1024);
    int live = 0, stale = 0;
    while (g_running) {
        t226_requests(root, &live, &stale);
        if (!ensure_writer_fifo(&g_writer)) continue;
        fifo_writer_write(&g_fifo, frame, size, g_frames.generation);
        usleep(50000);
    }
    free(frame);
    if (g_fifo.fd >= 0) close(g_fifo.fd);
    return 0;
}

int main(int argc, char **argv) {
    const char *fifo_fixture = getenv("USCREEN_T226_ROOT");
    if (fifo_fixture) return t226_command(argc, argv, fifo_fixture);
    const char *root = getenv("USCREEN_T330_DRM");
    if (root) return t330_command_lease(argc, argv, root);
    assert(argc == 2);
    static const struct { const char *id; void (*run)(void); } cases[] = {
        {"T343", test_t343},
        {"T341", test_t341},
        {"T340-missing", test_t340_missing},
        {"T340-directory", test_t340_directory},
        {"T340-empty", test_t340_empty},
        {"T340-oversized", test_t340_oversized},
        {"T340-readable", test_t340_readable},
        {"T324", test_t324},
        {"T294", test_t294},
        {"T293", test_t293},
        {"T290", test_t290},
        {"T279", test_t279},
        {"T274", test_t274},
        {"T272", test_t272},
        {"T315-channel", test_t315_channel},
        {"T315-mode", test_t315_mode},
        {"T315-signal", test_t315_signal},
        {"T254", test_t254},
        {"T170", test_helper_options},
        {"T108", test_t108},
        {"T082", test_t082},
        {"T113", test_t113},
        {"T226", test_t226},
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
