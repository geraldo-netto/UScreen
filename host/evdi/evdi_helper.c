#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <signal.h>
#include <poll.h>
#include <errno.h>
#include <time.h>
#include <dirent.h>
#include <pthread.h>
#include <sched.h>
#include <sys/stat.h>
#include <sys/file.h>
#include <fcntl.h>
#include <sys/mman.h>
#include <stdint.h>
#include <stdatomic.h>
#include <limits.h>
/* Only the public client API. The headers are upstream libevdi 1.15's, kept
   in sync with the library: the previous copies predated the
   ddcci_data_handler member of evdi_event_context, so a struct one pointer
   too short was being handed to a library that reads that member. */
#include "evdi_lib.h"
#include "conversion.h"
#include "frame_exchange.h"
#include "fifo_writer.h"
#include "capture.h"
#include "writer.h"

static frame_exchange_t g_frames = FRAME_EXCHANGE_INITIALIZER;

/* Lock-free atomics are safe in the signal handler and publish shutdown to
   both threads. volatile alone supplies neither ordering nor race safety. */
_Static_assert(ATOMIC_INT_LOCK_FREE == 2, "signal shutdown requires lock-free int atomics");
static atomic_int g_running = 1;
/* Written only by the event-loop thread; signals request a clean shutdown. */

static conv_pool_t g_conversion = CONV_POOL_INITIALIZER;
static capture_context_t g_capture = CAPTURE_INITIALIZER(&g_frames, &g_conversion, &g_running);

static fifo_writer_t g_fifo = FIFO_WRITER_INITIALIZER(&g_running, &g_frames.generation);

static writer_context_t g_writer = {&g_frames, &g_fifo, &g_running, 60};
static int g_pin_card = -1;
static int g_preferred_card = -1;
static int g_conversion_threads; /* zero preserves Auto's affinity budget */

static void handle_signal(int sig) {
    (void)sig;
    g_running = 0;
}

/* Large CPU IDs require a dynamically sized mask even when only one CPU is
 * allowed. A fixed cpu_set_t can fail with EINVAL on large kernel masks. */
static long available_cpus(void) {
    for (int cpus = CPU_SETSIZE; cpus <= INT_MAX / 2; cpus *= 2) {
        size_t size = CPU_ALLOC_SIZE(cpus);
        cpu_set_t *allowed = CPU_ALLOC(cpus);
        if (!allowed) return 1;
        int status = sched_getaffinity(0, size, allowed);
        int error = errno;
        long count = status == 0 ? CPU_COUNT_S(size, allowed) : 0;
        CPU_FREE(allowed);
        if (status == 0) return count;
        if (error != EINVAL) return sysconf(_SC_NPROCESSORS_ONLN);
    }
    return 1;
}

static void conv_pool_init(void) {
    int requested = g_conversion_threads;
    if (!requested) {
        long cpus = available_cpus();
        if (cpus > MAX_CONV_THREADS + 2) cpus = MAX_CONV_THREADS + 2;
        requested = cpus > 2 ? (int)(cpus - 2) : 1;
    }
    conv_pool_start(&g_conversion, requested);
    fprintf(stderr, "[evdi-helper] Conversion capacity: policy=%s requested=%d effective=%d (including caller)\n",
            g_conversion_threads ? "manual" : "auto", requested, g_conversion.count);
}

static int choose_card_after(const char *name, int after, int found) {
    const char *digits = name + 4;
    char *end = NULL;
    long card = strtol(digits, &end, 10);
    if (end != digits && *end == '\0' && card > after && card <= INT_MAX
            && (found < 0 || card < found)) {
        found = (int)card;
    }    return found;
}

static int find_card_after(const char *drm_path, int after, int found) {
    DIR *drm_dir = opendir(drm_path);
    if (!drm_dir) return found;

    struct dirent *drm_entry;
    while ((drm_entry = readdir(drm_dir)) != NULL) {
        if (strncmp(drm_entry->d_name, "card", 4) != 0)
            continue;
        found = choose_card_after(drm_entry->d_name, after, found);
    }
    closedir(drm_dir);
    return found;
}

static int find_evdi_device_after(const char *root, int after) {
    DIR *dir = opendir(root);
    if (!dir) return -1;

    struct dirent *entry;
    int found = -1;
    while ((entry = readdir(dir)) != NULL) {
        if (strncmp(entry->d_name, "evdi.", 5) != 0)
            continue;

        char drm_path[4096];
        snprintf(drm_path, sizeof(drm_path), "%s/%s/drm", root, entry->d_name);

        found = find_card_after(drm_path, after, found);
    }
    closedir(dir);
    return found;
}

static int find_evdi_device_in(const char *root) {
    return find_evdi_device_after(root, -1);
}

static int connector_status_connected(const char *path, const char *name) {
    int connected = 0;
    char status_path[8192];
    snprintf(status_path, sizeof(status_path), "%s/%s/status", path, name);
    FILE *status = fopen(status_path, "r");
    if (status) {
        char value[32] = {0};
        if (fgets(value, sizeof(value), status) && strcmp(value, "connected\n") == 0) connected = 1;
        fclose(status);
    }    return connected;
}

static int directory_has_connected_output(const char *path) {
    DIR *connectors = opendir(path);
    if (!connectors) return 0;
    int connected = 0;
    struct dirent *connector;
    while ((connector = readdir(connectors)) != NULL) {
        if (strncmp(connector->d_name, "card", 4) != 0 || !strchr(connector->d_name, '-')) continue;
        if (connector_status_connected(path, connector->d_name)) connected = 1;
    }
    closedir(connectors);
    return connected;
}

static int card_connected_in(const char *root, int card) {
    DIR *devices = opendir(root);
    if (!devices) return 0;
    int connected = 0;
    struct dirent *device;
    while (!connected && (device = readdir(devices)) != NULL) {
        if (strncmp(device->d_name, "evdi.", 5) != 0) continue;
        char path[4096];
        snprintf(path, sizeof(path), "%s/%s/drm/card%d", root, device->d_name, card);
        connected = directory_has_connected_output(path);
    }
    closedir(devices);
    return connected;
}

static evdi_handle open_available_device_in(const char *root, int pinned, int *index) {
    int card = pinned >= 0 ? pinned : find_evdi_device_in(root);
    while (card >= 0) {
        *index = card;
        if (!card_connected_in(root, card)) {
            evdi_handle handle = evdi_open(card);
            if (handle != EVDI_INVALID_HANDLE) {
                /* libevdi permits multiple opens. Hold a kernel-backed lease
                   on the DRM inode until evdi_close closes this handle. */
                if (flock(evdi_get_event_ready(handle), LOCK_EX | LOCK_NB) == 0 &&
                        !card_connected_in(root, card)) return handle;
                evdi_close(handle);
            }
        }
        if (pinned >= 0) break;
        card = find_evdi_device_after(root, card);
    }
    return EVDI_INVALID_HANDLE;
}

/* Automatic sessions keep their previous card when it is still free. A
   strict --card pin never falls back; all paths use the same exclusive lease. */
static evdi_handle open_session_device_in(const char *root, int pinned, int preferred, int *index) {
    if (pinned >= 0) return open_available_device_in(root, pinned, index);
    if (preferred >= 0 && find_evdi_device_after(root, preferred - 1) == preferred) {
        evdi_handle handle = open_available_device_in(root, preferred, index);
        if (handle != EVDI_INVALID_HANDLE) return handle;
    }
    return open_available_device_in(root, -1, index);
}

static int request_evdi_device(void) {
    int written = evdi_add_device();
    if (written <= 0) {
        fprintf(stderr, "[evdi-helper] Failed to add EVDI device (result=%d); check module and sysfs permissions with uscreen doctor\n", written);
        return 0;
    }
    return 1;
}

static evdi_handle wait_for_available_device(const char *root, int timeout_ms, int *index) {
    for (int waited = 0; waited < timeout_ms && g_running; waited += 100) {
        evdi_handle handle = open_available_device_in(root, -1, index);
        if (handle != EVDI_INVALID_HANDLE) return handle;
        usleep(100000);
    }
    return EVDI_INVALID_HANDLE;
}

typedef struct {
    const char *edid_path;
    const char *fifo_path;
} helper_options_t;

static int set_numeric_option(const char *name, const char *value) {
    if (strcmp(name, "--scale") == 0) {
        g_capture.scale = atoi(value);
        if (g_capture.scale < 1) g_capture.scale = 1;
        if (g_capture.scale > 4) g_capture.scale = 4;
    } else if (strcmp(name, "--card") == 0) {
        /* Pin the assigned card; never borrow another tablet's slot. */
        g_pin_card = atoi(value);
    } else if (strcmp(name, "--preferred-card") == 0) {
        g_preferred_card = atoi(value);
    } else if (strcmp(name, "--fps") == 0) {
        g_capture.fps = atoi(value);
        if (g_capture.fps < 1 || g_capture.fps > 240) g_capture.fps = 60;
    } else {
        return 0;
    }
    return 1;
}

static int conversion_capacity(const char *value) {
    char *end;
    errno = 0;
    long count = strtol(value, &end, 10);
    return !errno && end != value && *end == '\0' && count >= 0 && count <= MAX_CONV_THREADS ? (int)count : 0;
}

static int set_helper_option(helper_options_t *options, const char *name, const char *value) {
    if (strcmp(name, "--edid") == 0) options->edid_path = value;
    else if (strcmp(name, "--capture-fifo") == 0) options->fifo_path = value;
    else if (strcmp(name, "--pipe-size-file") == 0) g_fifo.capacity_path = value;
    else if (strcmp(name, "--conversion-threads") == 0) g_conversion_threads = conversion_capacity(value);
    else return set_numeric_option(name, value);
    return 1;
}

static helper_options_t parse_helper_options(int argc, char *argv[]) {
    helper_options_t options = {0};
    for (int i = 1; i < argc; i++) {
        if (i + 1 < argc && set_helper_option(&options, argv[i], argv[i + 1])) i++;
    }
    return options;
}

static void initialize_helper_runtime(void) {
    frame_exchange_init(&g_frames);

    struct sigaction sa;
    memset(&sa, 0, sizeof(sa));
    sa.sa_handler = handle_signal;
    sigaction(SIGINT, &sa, NULL);
    sigaction(SIGTERM, &sa, NULL);
    signal(SIGPIPE, SIG_IGN);

}

static evdi_handle acquire_capture_device_in(const char *root, int *index) {
    /* Reuse an existing EVDI device if one is free (e.g. from a previous
       run) — adding a new DRM card on every restart floods the compositor
       with display hotplug events. */
    evdi_handle handle = open_session_device_in(root, g_pin_card, g_preferred_card, index);
    if (handle != EVDI_INVALID_HANDLE) {
        fprintf(stderr, "[evdi-helper] Reusing EVDI device /dev/dri/card%d\n", (*index));
    }

    if (handle == EVDI_INVALID_HANDLE) {
        if (g_pin_card >= 0) {
            fprintf(stderr, "[evdi-helper] Assigned card%d is unavailable; refusing another slot's card\n", g_pin_card);
            return EVDI_INVALID_HANDLE;
        }
        fprintf(stderr, "[evdi-helper] Creating EVDI device...\n");
        if (!request_evdi_device()) return EVDI_INVALID_HANDLE;

        fprintf(stderr, "[evdi-helper] Waiting for EVDI device...\n");
        handle = wait_for_available_device(root, 5000, index);
        if (handle == EVDI_INVALID_HANDLE) {
            fprintf(stderr, "[evdi-helper] No free EVDI device appeared within timeout.\n"
                            "[evdi-helper] Either the evdi kernel module is not loaded, or no device exists\n"
                            "[evdi-helper] and /sys/devices/evdi/add is root-only. Check `lsmod | grep evdi`;\n"
                            "[evdi-helper] then, once: echo 'options evdi initial_device_count=2' | sudo tee /etc/modprobe.d/uscreen-evdi.conf\n"
                            "[evdi-helper]            sudo modprobe evdi; echo 1 | sudo tee /sys/devices/evdi/add\n"
                            "[evdi-helper] Keep the live module loaded. If adding fails, reboot after checking evdi-dkms; see docs/installation.md.\n");
            return EVDI_INVALID_HANDLE;
        }
        fprintf(stderr, "[evdi-helper] Found EVDI device at /dev/dri/card%d\n", (*index));
    }
    return handle;
}

static evdi_handle acquire_capture_device(int *index) {
    return acquire_capture_device_in("/sys/devices/platform", index);
}

static FILE *open_edid_file(const char *path) {
    /* Do not wait for a FIFO writer before discovering it cannot be sought. */
    int fd = open(path, O_RDONLY | O_NONBLOCK | O_CLOEXEC);
    if (fd < 0) return NULL;
    struct stat info;
    if (fstat(fd, &info) != 0 || !S_ISREG(info.st_mode)) {
        close(fd);
        return NULL;
    }
    FILE *file = fdopen(fd, "rb");
    if (!file) close(fd);
    return file;
}

static unsigned char *read_edid_file(const char *edid_path, long *size) {
    FILE *f = open_edid_file(edid_path);
    if (!f) {
        fprintf(stderr, "[evdi-helper] Failed to open EDID file: %s\n", edid_path);
        return NULL;
    }
    fseek(f, 0, SEEK_END);
    long edid_size = ftell(f);
    if (edid_size <= 0 || edid_size > 32768) {
        fprintf(stderr, "[evdi-helper] Invalid EDID size: %ld\n", edid_size);
        fclose(f);
        return NULL;
    }
    fseek(f, 0, SEEK_SET);
    unsigned char *edid = malloc((size_t)edid_size);
    if (!edid) {
        fprintf(stderr, "[evdi-helper] Failed to allocate EDID buffer\n");
        fclose(f);
        return NULL;
    }
    size_t read_bytes = fread(edid, 1, (size_t)edid_size, f);
    fclose(f);
    if ((long)read_bytes != edid_size) {
        fprintf(stderr, "[evdi-helper] EDID read error: got %zu of %ld bytes\n", read_bytes, edid_size);
        free(edid);
        return NULL;
    }

    *size = edid_size;
    return edid;
}

static int start_capture_writer(const char *fifo_path, pthread_t *writer) {
    if (fifo_path) {
        g_fifo.path = fifo_path;
        g_writer.fps = g_capture.fps;
        conv_pool_init();   /* spawn NV12 conversion workers before first grab */
        fprintf(stderr, "[evdi-helper] Capture FIFO: %s (opened on demand)\n", fifo_path);
        if (pthread_create(writer, NULL, writer_run, &g_writer) != 0) {
            fprintf(stderr, "[evdi-helper] Failed to start writer thread\n");
            return 0;
        }
    }

    return 1;
}

static void shutdown_capture(evdi_handle handle, pthread_t writer) {
    g_running = 0;
    /* Wake the writer out of its condition wait so shutdown is immediate
       rather than up to one frame period late. */
    pthread_mutex_lock(&g_frames.mutex);
    pthread_cond_broadcast(&g_frames.ready);
    pthread_mutex_unlock(&g_frames.mutex);
    if (writer) pthread_join(writer, NULL);
    conv_pool_destroy(&g_conversion);
    fifo_writer_close(&g_fifo);
    free(g_capture.framebuffer);
    frame_exchange_free(&g_frames);

    fprintf(stderr, "[evdi-helper] Disconnecting...\n");
    evdi_disconnect(handle);
    evdi_close(handle);
    g_capture.handle = EVDI_INVALID_HANDLE;

    fprintf(stderr, "[evdi-helper] Done.\n");
}

static int run_capture(evdi_handle handle, pthread_t writer) {
    int status = capture_run(&g_capture, handle);
    shutdown_capture(handle, writer);
    return status;
}

int main(int argc, char *argv[]) {
    helper_options_t options = parse_helper_options(argc, argv);
    const char *edid_path = options.edid_path;
    const char *fifo_path = options.fifo_path;

    if (!edid_path) {
        fprintf(stderr, "Usage: %s --edid <edid.bin> [--capture-fifo <path>] [--fps <n>] [--scale <1-4>] [--pipe-size-file <path>] [--conversion-threads <0-128>]\n", argv[0]);
        return 1;
    }

    initialize_helper_runtime();

    /* File errors must fail before opening DRM cards or creating devices. */
    long edid_size;
    unsigned char *edid = read_edid_file(edid_path, &edid_size);
    if (!edid) return 1;

    int dev_idx = -1;
    evdi_handle handle = acquire_capture_device(&dev_idx);
    if (handle == EVDI_INVALID_HANDLE) {
        free(edid);
        return 1;
    }
    g_capture.device_index = dev_idx;
    g_capture.handle = handle;

    fprintf(stderr, "[evdi-helper] Connecting with EDID (%ld bytes)...\n", edid_size);
    evdi_connect(handle, edid, (unsigned int)edid_size, 0);
    free(edid);

    printf("EVDI_CONNECTED card%d\n", dev_idx);
    fflush(stdout);

    pthread_t writer = 0;
    if (!start_capture_writer(fifo_path, &writer)) return 1;

    fprintf(stderr, "[evdi-helper] Connected. Capture at %d fps. Entering event loop.\n", g_capture.fps);
    return run_capture(handle, writer);
}
