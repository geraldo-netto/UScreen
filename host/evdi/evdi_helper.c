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

static evdi_handle g_handle = EVDI_INVALID_HANDLE;
static int g_device_index = -1;
/* Lock-free atomics are safe in the signal handler and publish shutdown to
   both threads. volatile alone supplies neither ordering nor race safety. */
_Static_assert(ATOMIC_INT_LOCK_FREE == 2, "signal shutdown requires lock-free int atomics");
static atomic_int g_running = 1;

static int g_capture_fifo_fd = -1;
static const char *g_fifo_path = NULL;
static int g_fps = 60;

/* EVDI-registered framebuffer (stride-padded, written by the kernel) */
static unsigned char *g_framebuffer = NULL;
static int g_fb_size = 0;
static int g_mode_w = 0;
static int g_mode_h = 0;
static int g_mode_bpp = 4;
static int g_mode_stride = 0;
static volatile int g_have_mode = 0;
static int g_pin_card = -1;
static int g_dpms_on = 0;

/* Triple buffer for FIFO writes: grabber packs into g_fill, swaps with
   g_latest; writer swaps g_latest into g_write and streams it out.
   Pointer swaps only — no copies between threads, no stalls. */
static pthread_mutex_t g_swap_mutex = PTHREAD_MUTEX_INITIALIZER;
/* Signalled by publish_frame() so the writer wakes the moment a frame exists
   rather than on its own timer — see writer_thread(). */
/* Initialised in main() against CLOCK_MONOTONIC.
   NOT PTHREAD_COND_INITIALIZER: that condvar measures absolute timeouts
   against CLOCK_REALTIME, while the deadlines here come from
   CLOCK_MONOTONIC. Monotonic time is seconds-since-boot and realtime is
   seconds-since-1970, so every deadline looked decades overdue,
   pthread_cond_timedwait returned ETIMEDOUT immediately and the writer
   thread spun at ~65% of a core doing nothing. */
static pthread_cond_t g_frame_ready;
static unsigned char *g_fill = NULL;
static unsigned char *g_latest = NULL;
static unsigned char *g_write = NULL;
static volatile int g_latest_valid = 0;

/* Per-buffer "which chroma rows are out of date in THIS buffer", one bit per
   chroma row. Converting the whole frame every time is wasted work: a desktop
   typically changes a small band, and EVDI already tells us which.
   Why per buffer rather than one global list: with triple buffering each
   buffer was last filled at a different moment, so each is stale in a
   different set of rows. A single shared list would leave whichever buffer
   was skipped holding torn, half-updated content. Damage is therefore
   recorded into all three, and cleared only for the buffer just converted.
   The masks swap together with the buffer pointers they describe. */
static unsigned char *g_dirty_fill = NULL;
static unsigned char *g_dirty_latest = NULL;
static unsigned char *g_dirty_write = NULL;
static int g_dirty_bytes = 0;      /* size of one mask */
static int g_chroma_rows = 0;      /* mode height / 2 */
static int g_packed_size = 0;          /* NV12 frame: out_w*out_h*3/2 */

/* Integer downscale applied on the way to the encoder. The desktop keeps its
   native mode — window layout and scaling are unaffected — while the stream
   carries fewer pixels. That cuts both the bytes crossing into the encoder and
   the tablet's decode time, which measurements put at roughly 7-8ms fixed plus
   1.2ms per megapixel. An integer divisor means the tablet upscales by a whole
   number, avoiding resampling artefacts on top of the softness.
   1 = native, and keeps the original tight conversion loop. */
static int g_scale = 1;
static int g_out_w = 0;
static int g_out_h = 0;
static volatile int g_buffers_ready = 0;

/* Re-send an unchanged screen at 5 fps. This keeps the client's read timeout
   alive and supplies frames for the CLI's one-second wall-clock IDR schedule.
   The optional in-process encoder can also honor a join request on the next
   frame. Both paths need this idle input without paying the full target fps. */
#define IDLE_KEEPALIVE_MS 200
static long long g_last_write_ms = 0;

static long long now_us(void);
/* Capture-side latency: grab → NV12 → handed to the encoder's FIFO.
   Reported as percentiles so the cost of the pipe-to-ffmpeg design can be
   compared against the decode and wire costs measured on the other side. */
#define LAT_SAMPLES 256
static int g_lat_us[LAT_SAMPLES];
static int g_lat_count = 0;
static long long g_grab_us = 0;      /* when the frame now in g_fill was grabbed */
static long long g_latest_grab_us = 0;
static long long g_write_grab_us = 0;

static int cmp_int(const void *a, const void *b) {
    int x = *(const int *)a, y = *(const int *)b;
    return (x > y) - (x < y);
}

static volatile int g_update_pending = 0;  /* request_update sent, waiting for update_ready */
static atomic_int g_writer_busy = 0;     /* writer is streaming g_write to the FIFO */
static atomic_uint g_mode_generation = 0; /* bumped on every mode change */
static volatile long long g_grab_count = 0;

/* Where the capture cycle spends its time, printed with the 5s stats. The
   two halves of the cycle are the wait for the compositor to answer a
   request and the copy out of the framebuffer; knowing both is what tells
   a slow compositor from a slow helper. */
static long long g_req_us = 0;          /* when the outstanding request was sent */
static long long g_wait_sum_us = 0;     /* request → update_ready */
static int g_wait_n = 0;
static long long g_grab_sum_us = 0;     /* evdi_grab_pixels duration */
static int g_grab_n = 0;
static int g_immediate_n = 0;           /* requests the kernel answered at once */
static int g_empty_n = 0;               /* grabs that returned no damaged rects */
/* Request the next frame as soon as the previous one has been grabbed
   instead of waiting for the next period tick. USCREEN_NO_PIPELINE=1 restores
   the strictly paced cycle for comparison. */
static int g_pipeline = 1;
static volatile long long g_last_request_ms = 0;

static void handle_signal(int sig) {
    (void)sig;
    g_running = 0;
}

static long long now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long long)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

static long long now_us(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long long)ts.tv_sec * 1000000 + ts.tv_nsec / 1000;
}

static void on_dpms(int dpms_mode, void *user_data) {
    (void)user_data;
    g_dpms_on = (dpms_mode == 0) ? 1 : 0;
    fprintf(stderr, "[evdi-helper] DPMS: %d (%s)\n", dpms_mode, g_dpms_on ? "ON" : "OFF");
}

/* --- BGRA -> NV12 conversion -------------------------------------------
   The kernel hands us a 32-bit BGRA framebuffer (21.9 MB at 2960x1848).
   Feeding that raw to the encoder is memory-bandwidth bound: it gets copied
   three times per frame (pack + pipe write + pipe read), which caps the
   pipeline around 70 fps even though NVENC itself sits near-idle. NV12 is
   1.5 bytes/px (8.2 MB) — 2.7x less data through every stage — and NVENC
   accepts it natively, so the encoder no longer color-converts either.

   Conversion uses BT.709 limited-range coefficients (matching the bt709 and
   `-color_range tv` tags the encoder writes) and is split across a small
   thread pool so it costs ~1 ms rather than stalling the capture loop.

   Full range was tried and reverted. It is theoretically better — 256 levels
   instead of 220 — but this tablet's decoder does not act on the SPS
   full-range flag even when the format also declares COLOR_RANGE_FULL: it
   assumes limited range, stretches the levels downwards and visibly crushes
   shadows. Limited range costs precision, not range (the decoder expands
   16-235 back to 0-255), so the only real exposure is slight banding in
   gradients, which is the lesser problem by a wide margin. */

typedef struct {
    const unsigned char *src;   /* BGRA, stride-padded */
    unsigned char *ydst;        /* Y plane, w bytes/row */
    unsigned char *uvdst;       /* interleaved CbCr, w bytes per chroma row */
    int w, h, stride;           /* source dimensions */
    int ow, oh;                 /* destination dimensions (w/scale, h/scale) */
    int scale;                  /* 1 = no downscale */
    int cy0, cy1;               /* chroma-row range [cy0, cy1) this job owns */
    const unsigned char *dirty; /* NULL = convert everything */
} conv_job_t;

static inline int row_is_dirty(const unsigned char *mask, int cy) {
    return mask == NULL || (mask[cy >> 3] & (1u << (cy & 7)));
}

static inline unsigned char clamp_byte(int value) {
    if (value < 0) return 0;
    if (value > 255) return 255;
    return (unsigned char)value;
}

/* BT.709 limited-range chroma from the sum of four luma samples. */
static inline void write_chroma(unsigned char *uv, int sb, int sg, int sr) {
    int ab = sb >> 2, ag = sg >> 2, ar = sr >> 2;
    uv[0] = clamp_byte(((-26 * ar - 86 * ag + 112 * ab + 128) >> 8) + 128);
    uv[1] = clamp_byte(((112 * ar - 102 * ag - 10 * ab + 128) >> 8) + 128);
}

/* Downscaling variant: every output pixel is the mean of a scale x scale
   source block, and each chroma sample the mean of the 2*scale square it
   covers. Box averaging rather than point sampling — dropping pixels would
   alias hard on desktop content, where single-pixel lines are everywhere. */
static inline void convert_strip_scaled(const conv_job_t *j) {
    const int n = j->scale, stride = j->stride, ow = j->ow;
    const int inv = n * n;
    for (int cy = j->cy0; cy < j->cy1; cy++) {
        if (!row_is_dirty(j->dirty, cy))
            continue;
        unsigned char *yo0 = j->ydst + (size_t)(cy * 2) * ow;
        unsigned char *yo1 = yo0 + ow;
        unsigned char *uv  = j->uvdst + (size_t)cy * ow;
        for (int ox = 0; ox < ow; ox += 2) {
            int csb = 0, csg = 0, csr = 0;      /* chroma: whole 2n x 2n block */
            for (int q = 0; q < 4; q++) {       /* four output luma pixels */
                int oxx = ox + (q & 1);
                int oyy = cy * 2 + (q >> 1);
                int sb = 0, sg = 0, sr = 0;
                for (int dy = 0; dy < n; dy++) {
                    const unsigned char *row =
                        j->src + (size_t)(oyy * n + dy) * stride + (size_t)(oxx * n) * 4;
                    for (int dx = 0; dx < n; dx++) {
                        sb += row[dx * 4];
                        sg += row[dx * 4 + 1];
                        sr += row[dx * 4 + 2];
                    }
                }
                int b = sb / inv, g = sg / inv, r = sr / inv;
                unsigned char yv =
                    (unsigned char)(((47 * r + 157 * g + 16 * b + 128) >> 8) + 16);
                if (q < 2) yo0[oxx] = yv; else yo1[oxx] = yv;
                csb += b; csg += g; csr += r;
            }
            write_chroma(uv + ox, csb, csg, csr);
        }
    }
}

static inline void convert_strip(const conv_job_t *j) {
    const int w = j->ow, stride = j->stride;
    for (int cy = j->cy0; cy < j->cy1; cy++) {
        if (!row_is_dirty(j->dirty, cy))
            continue;
        int y0 = cy * 2, y1 = y0 + 1;
        const unsigned char *row0 = j->src + (size_t)y0 * stride;
        const unsigned char *row1 = j->src + (size_t)y1 * stride;
        unsigned char *yo0 = j->ydst + (size_t)y0 * w;
        unsigned char *yo1 = j->ydst + (size_t)y1 * w;
        unsigned char *uv  = j->uvdst + (size_t)cy * w;
        for (int x = 0; x < w; x += 2) {
            const unsigned char *p;
            int b, g, r, sb, sg, sr;
            p = row0 + (size_t)x * 4;       b = p[0]; g = p[1]; r = p[2];
            yo0[x]   = (unsigned char)(((47*r + 157*g + 16*b + 128) >> 8) + 16);
            sb = b; sg = g; sr = r;
            p = row0 + (size_t)(x+1) * 4;   b = p[0]; g = p[1]; r = p[2];
            yo0[x+1] = (unsigned char)(((47*r + 157*g + 16*b + 128) >> 8) + 16);
            sb += b; sg += g; sr += r;
            p = row1 + (size_t)x * 4;       b = p[0]; g = p[1]; r = p[2];
            yo1[x]   = (unsigned char)(((47*r + 157*g + 16*b + 128) >> 8) + 16);
            sb += b; sg += g; sr += r;
            p = row1 + (size_t)(x+1) * 4;   b = p[0]; g = p[1]; r = p[2];
            yo1[x+1] = (unsigned char)(((47*r + 157*g + 16*b + 128) >> 8) + 16);
            sb += b; sg += g; sr += r;
            write_chroma(uv + x, sb, sg, sr);
        }
    }
}

#define MAX_CONV_THREADS 8
static int g_nthreads = 1;
static pthread_t g_pool[MAX_CONV_THREADS];
static conv_job_t g_jobs[MAX_CONV_THREADS];
static pthread_mutex_t g_pool_mtx = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t g_pool_go = PTHREAD_COND_INITIALIZER;
static pthread_cond_t g_pool_done = PTHREAD_COND_INITIALIZER;
static unsigned int g_pool_gen = 0; /* frame generation; unsigned wrap is defined */
static int g_pool_active = 0;    /* worker jobs still running this gen */
static int g_pool_shutdown = 0;

static void *conv_worker(void *arg) {
    int id = (int)(intptr_t)arg;
    unsigned int last_gen = 0;
    for (;;) {
        pthread_mutex_lock(&g_pool_mtx);
        while (g_pool_gen == last_gen && !g_pool_shutdown)
            pthread_cond_wait(&g_pool_go, &g_pool_mtx);
        if (g_pool_shutdown) { pthread_mutex_unlock(&g_pool_mtx); break; }
        last_gen = g_pool_gen;
        pthread_mutex_unlock(&g_pool_mtx);

        if (g_jobs[id].scale > 1) convert_strip_scaled(&g_jobs[id]);
        else convert_strip(&g_jobs[id]);

        pthread_mutex_lock(&g_pool_mtx);
        if (--g_pool_active == 0)
            pthread_cond_signal(&g_pool_done);
        pthread_mutex_unlock(&g_pool_mtx);
    }
    return NULL;
}

static void conv_pool_init(void) {
    long n = sysconf(_SC_NPROCESSORS_ONLN);
    g_nthreads = (int)(n - 2);              /* leave cores for ffmpeg/KWin */
    if (g_nthreads < 1) g_nthreads = 1;
    if (g_nthreads > MAX_CONV_THREADS) g_nthreads = MAX_CONV_THREADS;
    /* Worker threads handle jobs 1..n-1; the caller runs job 0 itself. */
    for (int i = 1; i < g_nthreads; i++) {
        int error = pthread_create(&g_pool[i], NULL, conv_worker, (void *)(intptr_t)i);
        if (error != 0) {
            fprintf(stderr, "[evdi-helper] Conversion worker unavailable: %s\n", strerror(error));
            g_nthreads = i;
            break;
        }
    }
    fprintf(stderr, "[evdi-helper] NV12 conversion using %d thread(s)\n", g_nthreads);
}

static void bgra_to_nv12(const unsigned char *src, unsigned char *dst,
                         const unsigned char *dirty) {
    int ch = g_out_h / 2;
    unsigned char *ydst = dst;
    unsigned char *uvdst = dst + (size_t)g_out_w * g_out_h;
    for (int i = 0; i < g_nthreads; i++) {
        g_jobs[i] = (conv_job_t){ src, ydst, uvdst, g_mode_w, g_mode_h, g_mode_stride,
                                  g_out_w, g_out_h, g_scale,
                                  ch * i / g_nthreads, ch * (i + 1) / g_nthreads,
                                  dirty };
    }
    if (g_nthreads > 1) {
        pthread_mutex_lock(&g_pool_mtx);
        g_pool_active = g_nthreads - 1;
        g_pool_gen++;
        pthread_cond_broadcast(&g_pool_go);
        pthread_mutex_unlock(&g_pool_mtx);
    }
    if (g_jobs[0].scale > 1) convert_strip_scaled(&g_jobs[0]);
    else convert_strip(&g_jobs[0]);   /* caller does its own strip */
    if (g_nthreads > 1) {
        pthread_mutex_lock(&g_pool_mtx);
        while (g_pool_active > 0)
            pthread_cond_wait(&g_pool_done, &g_pool_mtx);
        pthread_mutex_unlock(&g_pool_mtx);
    }
}

static void mark_all_dirty(void) {
    if (!g_dirty_fill) return;
    memset(g_dirty_fill,   0xFF, (size_t)g_dirty_bytes);
    memset(g_dirty_latest, 0xFF, (size_t)g_dirty_bytes);
    memset(g_dirty_write,  0xFF, (size_t)g_dirty_bytes);
}

/* Record damaged chroma rows into every buffer's mask: each of them now lacks
   this content until it is individually reconverted. */
static void mark_damage(const struct evdi_rect *rects, int n) {
    if (!g_dirty_fill || g_chroma_rows <= 0) return;
    for (int i = 0; i < n; i++) {
        int y0 = rects[i].y1, y1 = rects[i].y2;
        if (y1 < y0) { int t = y0; y0 = y1; y1 = t; }
        /* Source rows map onto output chroma rows through the scale: one
           chroma row covers 2*scale source rows. */
        int div = 2 * g_scale;
        int c0 = y0 / div, c1 = (y1 + div - 1) / div;
        if (c0 < 0) c0 = 0;
        if (c1 > g_chroma_rows) c1 = g_chroma_rows;
        for (int cy = c0; cy < c1; cy++) {
            unsigned char bit = (unsigned char)(1u << (cy & 7));
            g_dirty_fill[cy >> 3]   |= bit;
            g_dirty_latest[cy >> 3] |= bit;
            g_dirty_write[cy >> 3]  |= bit;
        }
    }
}

/* Convert the stride-padded BGRA framebuffer into the NV12 fill buffer and
   publish it as the latest frame. Called from the event-loop thread. */
static void publish_frame(void) {
    if (!g_buffers_ready || !g_framebuffer)
        return;

    /* Only the rows this particular buffer is missing. */
    bgra_to_nv12(g_framebuffer, g_fill, g_dirty_fill);
    memset(g_dirty_fill, 0, (size_t)g_dirty_bytes);

    pthread_mutex_lock(&g_swap_mutex);
    unsigned char *tmp = g_latest;
    g_latest = g_fill;
    g_fill = tmp;
    /* The masks describe the buffers, so they travel with them. */
    unsigned char *dtmp = g_dirty_latest;
    g_dirty_latest = g_dirty_fill;
    g_dirty_fill = dtmp;
    g_latest_valid = 1;
    g_latest_grab_us = g_grab_us;
    pthread_cond_signal(&g_frame_ready);
    pthread_mutex_unlock(&g_swap_mutex);
}

static int g_buffer_registered = 0;

static void reject_mode(void) {
    g_have_mode = 0;
    pthread_mutex_lock(&g_swap_mutex);
    g_buffers_ready = 0;
    pthread_mutex_unlock(&g_swap_mutex);
    g_running = 0;
}

static int validate_frame_format(struct evdi_mode mode) {
    /* Both conversion paths read little-endian XRGB8888/ARGB8888 as BGRA.
       Stop before registering or reading any buffer with another layout. */
    if (mode.bits_per_pixel != 32 ||
            (mode.pixel_format != 0x34325258 && mode.pixel_format != 0x34325241)) {
        fprintf(stderr, "[evdi-helper] Unsupported framebuffer format; need XRGB8888 or ARGB8888\n");
        reject_mode();
        return 0;
    }
    return 1;
}

static int validate_mode_dimensions(struct evdi_mode mode) {
    /* Each output chroma sample reads a complete 2*scale source block.
       Enlarging a tiny output to 2x2 would read outside the source image. */
    if (g_scale < 1 || g_scale > 4 || mode.width < 2 * g_scale || mode.height < 2 * g_scale ||
            mode.width > (INT_MAX - 63) / 4 ||
            (((long long)mode.width * 4 + 63) & ~63LL) * mode.height > INT_MAX) {
        fprintf(stderr, "[evdi-helper] Mode dimensions cannot be safely converted at scale %d\n", g_scale);
        reject_mode();
        return 0;
    }
    return 1;
}

static int configure_mode_geometry(struct evdi_mode mode) {
    int new_w = mode.width;
    int new_h = mode.height;
    int new_bpp = mode.bits_per_pixel / 8;
    if (new_bpp < 1) new_bpp = 4;

    /* Same geometry (KWin re-applying the mode)? Keep everything as-is.
       Re-registering on every event is what crashed libevdi before. */
    if (g_buffer_registered && new_w == g_mode_w && new_h == g_mode_h
            && new_bpp == g_mode_bpp) {
        fprintf(stderr, "[evdi-helper] Mode unchanged, keeping buffer\n");
        g_update_pending = 0;
        return 0;
    }

    g_mode_w = new_w;
    g_mode_h = new_h;
    g_mode_bpp = new_bpp;

    int row_bytes = g_mode_w * g_mode_bpp;
    int aligned_stride = (row_bytes + 63) & ~63;  /* DRM buffers are 64-byte aligned */
    g_mode_stride = aligned_stride;
    g_fb_size = g_mode_stride * g_mode_h;

    return 1;
}

static int retire_mode_buffers(void) {
    pthread_mutex_lock(&g_swap_mutex);
    g_latest_valid = 0;
    g_buffers_ready = 0;
    g_mode_generation++;
    pthread_mutex_unlock(&g_swap_mutex);

    /* Wait (bounded) for the writer to finish any in-flight write before
       freeing the buffer it's reading. Writer stalls are bounded because
       FIFO writes poll with a timeout, but never spin here forever — a
       stuck event loop blocks KWin's output handling. */
    for (int i = 0; i < 1000 && g_writer_busy; i++)
        usleep(1000);
    if (g_writer_busy) {
        fprintf(stderr, "[evdi-helper] Writer stuck during mode change — stopping capture\n");
        /* The writer may still hold the old size across its pacing sleep.
           Keep every buffer in place until shutdown joins it; replacing
           g_write here would pair that old size with a new allocation. */
        g_have_mode = 0;
        g_running = 0;
        return 0;
    }

    /* The kernel must drop its reference to the old framebuffer BEFORE we
       free it — freeing first is a use-after-free during a pending grab. */
    if (g_buffer_registered) {
        evdi_unregister_buffer(g_handle, 0);
        g_buffer_registered = 0;
    }
    return 1;
}

static void allocate_framebuffer(void) {
    free(g_framebuffer);
    /* The kernel copies the damaged part of the scanout buffer into this
       on every grab — the whole 22 MB when the compositor reports full
       damage, which KWin does for this output. That copy is a large share
       of the capture cycle (measured 4-7 ms at 2960x1848), and copy_to_user
       into ordinary 4 KiB pages pays a TLB miss every page. Ask for
       transparent huge pages and touch the memory once now, so the copies
       run over 2 MiB mappings that are already faulted in. */
    {
        size_t huge = 2u << 20;
        size_t len = ((size_t)g_fb_size + huge - 1) / huge * huge;
        void *p = NULL;
        if (posix_memalign(&p, huge, len) == 0 && p) {
            madvise(p, len, MADV_HUGEPAGE);
            memset(p, 0, len);
            g_framebuffer = p;
        } else {
            g_framebuffer = malloc(g_fb_size);
        }
    }

}

static void allocate_stream_buffers(void) {
    /* Stream dimensions: source divided by the scale, forced even because
       NV12 chroma covers 2x2 luma samples. */
    g_out_w = (g_mode_w / g_scale) & ~1;
    g_out_h = (g_mode_h / g_scale) & ~1;
    printf("STREAM_SIZE %d %d\n", g_out_w, g_out_h);
    fflush(stdout);

    /* Packed buffers hold NV12 (Y plane + half-size interleaved CbCr). */
    g_packed_size = g_out_w * g_out_h * 3 / 2;
    free(g_fill);   g_fill = malloc(g_packed_size);
    free(g_latest); g_latest = malloc(g_packed_size);
    free(g_write);  g_write = malloc(g_packed_size);

    /* Fresh buffers hold nothing, so every row is stale in all of them. */
    g_chroma_rows = g_out_h / 2;
    g_dirty_bytes = (g_chroma_rows + 7) / 8;
    free(g_dirty_fill);   g_dirty_fill   = malloc((size_t)g_dirty_bytes);
    free(g_dirty_latest); g_dirty_latest = malloc((size_t)g_dirty_bytes);
    free(g_dirty_write);  g_dirty_write  = malloc((size_t)g_dirty_bytes);
    if (g_dirty_fill && g_dirty_latest && g_dirty_write)
        mark_all_dirty();

}

static int mode_buffers_allocated(void) {
    return g_framebuffer && g_fill && g_latest && g_write
        && g_dirty_fill && g_dirty_latest && g_dirty_write;
}

static void on_mode_changed(struct evdi_mode mode, void *user_data) {
    (void)user_data;
    fprintf(stderr, "[evdi-helper] Mode: %dx%d@%dHz %dbpp fmt=0x%x\n",
            mode.width, mode.height, mode.refresh_rate,
            mode.bits_per_pixel, mode.pixel_format);
    if (!validate_frame_format(mode) || !validate_mode_dimensions(mode)) return;
    printf("MODE_CHANGED %d %d %d\n", mode.width, mode.height, mode.refresh_rate);
    fflush(stdout);

    if (!configure_mode_geometry(mode)) return;

    if (!retire_mode_buffers()) return;
    allocate_framebuffer();
    allocate_stream_buffers();

    if (!mode_buffers_allocated()) {
        fprintf(stderr, "[evdi-helper] Failed to allocate framebuffers\n");
        reject_mode();
        return;
    }

    /* Dark gray initial frame so the tablet shows something immediately */
    memset(g_framebuffer, 0x18, g_fb_size);

    struct evdi_buffer buf = {
        .id = 0,
        .buffer = g_framebuffer,
        .width = g_mode_w,
        .height = g_mode_h,
        .stride = g_mode_stride,
        .rects = NULL,
        .rect_count = 0,
    };
    evdi_register_buffer(g_handle, buf);
    g_buffer_registered = 1;

    pthread_mutex_lock(&g_swap_mutex);
    g_buffers_ready = 1;
    pthread_cond_signal(&g_frame_ready);
    pthread_mutex_unlock(&g_swap_mutex);
    publish_frame();

    g_have_mode = 1;
    g_update_pending = 0;
    fprintf(stderr, "[evdi-helper] Buffer 0 registered: %dx%d stride=%d (row_bytes=%d)\n",
            g_mode_w, g_mode_h, buf.stride, g_mode_w * g_mode_bpp);
}

static void grab_now(void) {
    struct evdi_rect rects[64];
    int num_rects = 64;
    long long t0 = now_us();
    evdi_grab_pixels(g_handle, rects, &num_rects);
    g_grab_sum_us += now_us() - t0;
    g_grab_n++;
    if (num_rects <= 0) g_empty_n++;
    if (num_rects > 0) {
        g_grab_count++;
        g_grab_us = now_us();
        /* Under the swap lock: the writer thread reassigns the mask pointers
           when it takes a frame, so touching them unlocked would race.
           If the driver returned more rectangles than we gave it room for,
           we cannot know what else changed — repaint everything. */
        pthread_mutex_lock(&g_swap_mutex);
        if (num_rects >= (int)(sizeof(rects) / sizeof(rects[0])))
            mark_all_dirty();
        else
            mark_damage(rects, num_rects);
        pthread_mutex_unlock(&g_swap_mutex);
        publish_frame();
    }
}

static void on_update_ready(int buffer_to_be_updated, void *user_data) {
    (void)user_data;
    (void)buffer_to_be_updated;
    if (g_update_pending) {
        g_wait_sum_us += now_us() - g_req_us;
        g_wait_n++;
    }
    g_update_pending = 0;
    grab_now();
    /* Ask for the next frame straight away rather than at the next tick of
       the target period. The cycle is serial by the driver's design — the
       compositor's flip only completes once we have copied the frame out,
       and it renders the next one after that — so at 2960x1848 the two
       halves (about 9 ms in KWin, 6 ms in the grab) already take a frame
       period or more. Gating the request on the period as well pushed the
       next request past the compositor's vblank slot, so a 30 fps target
       delivered 24 and a 90 fps target 50-ish. The mode's refresh rate is
       what paces the compositor; nothing here needs to hold it back. Asking
       first from inside this handler, before the grab, was tried and is
       worse (16 fps): the driver answers at once with the not-yet-grabbed
       damage and the cycle falls apart. */
    if (g_pipeline)
        g_last_request_ms = 0;
}

static void on_crtc_state(int state, void *user_data) {
    (void)user_data;
    fprintf(stderr, "[evdi-helper] CRTC state: %d\n", state);
}

static void on_cursor_set(struct evdi_cursor_set cursor_set, void *user_data) {
    (void)user_data;
    (void)cursor_set;
}

static void on_cursor_move(struct evdi_cursor_move cursor_move, void *user_data) {
    (void)user_data;
    (void)cursor_move;
}

/* (Re)open the capture FIFO without blocking forever: O_NONBLOCK open fails
   with ENXIO while no reader (ffmpeg) has the other end open. */
static int try_open_fifo(void) {
    /* O_NOFOLLOW: the path is in a private directory now, but a symlink
       planted there must still never redirect the screen into a file. */
    int fd = open(g_fifo_path, O_WRONLY | O_NONBLOCK | O_NOFOLLOW);
    if (fd < 0)
        return -1;
    /* Keep writes nonblocking: POLLOUT promises some space, not enough for
       an entire frame. The writer owns this fd until it closes/reopens it. */
    /* Enlarge the pipe so a full-frame write doesn't take hundreds of
       64KB round-trips with the encoder. */
    fcntl(fd, F_SETPIPE_SZ, 1 << 20);
    fprintf(stderr, "[evdi-helper] Capture FIFO opened\n");
    return fd;
}

/* Writer thread: sends the most recent frame, woken by the grabber rather than
   by a timer, and rate-limited so it never exceeds the target fps.

   The previous version free-ran on clock_nanosleep, entirely independent of
   when a frame was actually grabbed. A frame published just after a tick had to
   wait a whole period before being sent — half a frame of pure added latency on
   average, for nothing. Waiting on the condition variable removes that: the
   frame goes out as soon as it exists, and the minimum-interval check below
   still caps the rate.

   FIFO backpressure never stalls capture, and the FIFO is reopened
   automatically when the encoder restarts. */
enum fifo_wait_result { FIFO_STOP, FIFO_RETRY, FIFO_READY };

static enum fifo_wait_result wait_fifo_writable(long long deadline) {
    long long wait_ms = deadline - now_ms();
    if (wait_ms <= 0) return FIFO_STOP;
    struct pollfd wfd = { .fd = g_capture_fifo_fd, .events = POLLOUT };
    int pr = poll(&wfd, 1, wait_ms < 250 ? (int)wait_ms : 250);
    if (pr < 0 && errno == EINTR) return FIFO_RETRY;
    if (pr == 0 && now_ms() < deadline) return FIFO_RETRY;
    if (pr <= 0 || (wfd.revents & (POLLERR | POLLHUP | POLLNVAL))) return FIFO_STOP;
    return FIFO_READY;
}

static int fifo_write_retryable(ssize_t written) {
    return written < 0 && (errno == EINTR || errno == EAGAIN);
}

/* A live reader may stall briefly under load. Keep the same frame across
   poll timeouts, but bound a continuous stall and notice mode changes. */
static size_t write_fifo_frame(const unsigned char *ptr, size_t remaining) {
    long long deadline = now_ms() + 1000;
    unsigned generation = g_mode_generation;
    while (remaining > 0 && g_running && generation == g_mode_generation) {
        enum fifo_wait_result ready = wait_fifo_writable(deadline);
        if (ready == FIFO_RETRY) continue;
        if (ready == FIFO_STOP) break;
        ssize_t written = write(g_capture_fifo_fd, ptr, remaining);
        if (written <= 0) {
            if (fifo_write_retryable(written)) continue;
            break;
        }
        ptr += written;
        remaining -= (size_t)written;
        deadline = now_ms() + 1000;
    }
    if (remaining > 0) {
        fprintf(stderr, "[evdi-helper] Incomplete frame — closing FIFO to resync\n");
        close(g_capture_fifo_fd);
        g_capture_fifo_fd = -1;
    }
    return remaining;
}

static void record_latency(long long grab_us) {
    if (grab_us <= 0) return;
    long long d = now_us() - grab_us;
    pthread_mutex_lock(&g_swap_mutex);
    if (d >= 0 && d < 1000000 && g_lat_count < LAT_SAMPLES)
        g_lat_us[g_lat_count++] = (int)d;
    pthread_mutex_unlock(&g_swap_mutex);
}

typedef struct {
    long period_ns;
    struct timespec next_allowed;
    int have_frame;
    unsigned frame_generation;
} writer_state_t;

static void add_period(struct timespec *time, long period_ns) {
    time->tv_nsec += period_ns;
    while (time->tv_nsec >= 1000000000L) {
        time->tv_nsec -= 1000000000L;
        time->tv_sec += 1;
    }
}

static int ensure_writer_fifo(void) {
    if (g_capture_fifo_fd < 0) {
        g_capture_fifo_fd = try_open_fifo();
        if (g_capture_fifo_fd < 0) {
            /* No reader yet: poll slowly instead of spinning. */
            struct timespec idle = { .tv_sec = 0, .tv_nsec = 50000000L };
            nanosleep(&idle, NULL);
            return 0;
        }
    }
    return 1;
}

/* Caller holds g_swap_mutex; timedwait releases and reacquires it. */
static void wait_for_writer_frame(long period_ns) {
    /* Wait for a frame, but no longer than one period so shutdown and
       mode changes are still noticed promptly. */
    while (g_running && (!g_buffers_ready || !g_latest_valid)) {
        struct timespec wait_until;
        clock_gettime(CLOCK_MONOTONIC, &wait_until);
        add_period(&wait_until, period_ns);
        if (pthread_cond_timedwait(&g_frame_ready, &g_swap_mutex,
                                   &wait_until) == ETIMEDOUT)
            break;
    }
}

/* -1: stopped, 0: no frame, 1: writer owns the current buffer. */
static int claim_writer_frame(writer_state_t *state, int *size, int *fresh) {
    pthread_mutex_lock(&g_swap_mutex);
    wait_for_writer_frame(state->period_ns);
    if (!g_running) {
        pthread_mutex_unlock(&g_swap_mutex);
        return -1;
    }
    if (!g_buffers_ready) {
        pthread_mutex_unlock(&g_swap_mutex);
        return 0;
    }
    if (state->frame_generation != g_mode_generation) {
        /* Buffers were reallocated; previous g_write content is gone */
        state->frame_generation = g_mode_generation;
        state->have_frame = 0;
    }
    *fresh = 0;
    if (g_latest_valid) {
        unsigned char *tmp = g_write;
        g_write = g_latest;
        g_latest = tmp;
        unsigned char *dtmp = g_dirty_write;
        g_dirty_write = g_dirty_latest;
        g_dirty_latest = dtmp;
        g_latest_valid = 0;
        g_write_grab_us = g_latest_grab_us;
        state->have_frame = 1;
        *fresh = 1;
    }
    *size = g_packed_size;
    g_writer_busy = state->have_frame;
    pthread_mutex_unlock(&g_swap_mutex);

    return state->have_frame;
}

static int writer_frame_due(int fresh) {
    /* Nothing changed on screen: don't re-send the identical frame.
       A motionless desktop was still pushing 60 full NV12 frames a second
       through the FIFO — 8.2MB each, roughly half a gigabyte per second of
       pure memory traffic, plus an encode for every one of them, all to
       transmit no new information. The occasional keepalive keeps the
       encoder and the client's read timeout alive. */
    long long now_ms_write = now_ms();
    if (!fresh && (now_ms_write - g_last_write_ms) < IDLE_KEEPALIVE_MS) {
        pthread_mutex_lock(&g_swap_mutex);
        g_writer_busy = 0;
        pthread_mutex_unlock(&g_swap_mutex);
        return 0;
    }
    g_last_write_ms = now_ms_write;

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

static void *writer_thread(void *arg) {
    (void)arg;
    writer_state_t state = {
        .period_ns = 1000000000L / (g_fps > 0 ? g_fps : 60),
        .have_frame = 0,
        .frame_generation = UINT_MAX,
    };
    clock_gettime(CLOCK_MONOTONIC, &state.next_allowed);
    while (g_running) {
        if (!ensure_writer_fifo()) continue;
        int size, fresh;
        int claimed = claim_writer_frame(&state, &size, &fresh);
        if (claimed < 0) break;
        if (claimed == 0) continue;
        if (!writer_frame_due(fresh)) continue;
        pace_writer(&state);
        size_t remaining = write_fifo_frame(g_write, (size_t)size);
        g_writer_busy = 0;
        /* Repeated keepalives measure stale frame age, not capture latency. */
        if (fresh && remaining == 0) record_latency(g_write_grab_us);
    }
    return NULL;
}

static long capture_period_ms(void) {
    long request_period_ms = 1000 / (g_fps > 0 ? g_fps : 60);
    if (request_period_ms < 1) request_period_ms = 1;
    return request_period_ms;
}

static int capture_poll_timeout(long request_period_ms) {
    /* Sleep exactly until the next capture request is due instead of a
       fixed 4ms tick. The fixed tick quantised every request to a 4ms grid,
       adding up to 4ms of jitter per frame — a quarter of the entire budget
       at 60fps, and half of it at 120.
       With no mode there is nothing to request, and the deadline below
       would sit permanently in the past — poll would return instantly and
       the loop would spin at 100% of a core. Wait on events only. */
    long long timeout_ms;
    if (!g_have_mode) {
        timeout_ms = 100;
    } else if (g_update_pending) {
        /* Waiting for update_ready. The capture deadline below has already
           passed and cannot advance until the request completes, so
           deriving a timeout from it yields 0 forever and poll returns
           instantly — the loop then burns a whole core. This is not a
           rare state: disabling the virtual output leaves a mode set with
           nothing rendering to it, which is exactly what happens when the
           tablet is unplugged or switched to pen-only.
           Sleep until the watchdog is due instead. update_ready wakes poll
           the moment it arrives, so nothing is delayed when the
           compositor is actually running. */
        long long left = g_last_request_ms + 250 - now_ms();
        timeout_ms = left < 0 ? 0 : (left > 250 ? 250 : (int)left);
    } else {
        long long due = g_last_request_ms + request_period_ms;
        /* Keep overdue deadlines wide until bounded: pipeline callbacks reset
           the request time to zero, even after weeks of system uptime. */
        timeout_ms = due - now_ms();
        if (timeout_ms < 0) timeout_ms = 0;
        if (timeout_ms > 4) timeout_ms = 4;   /* stay responsive to events */
    }

    return (int)timeout_ms;
}

static void request_capture_if_due(evdi_handle handle, long long now, long request_period_ms) {
    /* Core capture cycle: request a fresh frame from the compositor at
       the target fps. If the kernel says pixels are ready right away,
       grab immediately; otherwise update_ready will fire and grab. */
    if (!g_update_pending && (now - g_last_request_ms) >= request_period_ms) {
        g_last_request_ms = now;
        g_req_us = now_us();
        if (evdi_request_update(handle, 0)) {
            g_immediate_n++;
            grab_now();
        } else {
            g_update_pending = 1;
        }
    }

}

static void recover_capture_if_stalled(long long now, long long *last_fallback_grab_ms) {
    /* Watchdog: if a request got lost (compositor hiccup), don't stay
       stuck waiting for update_ready forever. */
    if (g_update_pending && (now - g_last_request_ms) >= 250) {
        g_update_pending = 0;
        grab_now();
    }

    /* Fallback grab once a second in case no events flow at all */
    if ((now - (*last_fallback_grab_ms)) >= 1000) {
        (*last_fallback_grab_ms) = now;
        if (!g_update_pending)
            grab_now();
    }

}

static void report_capture_stats(long long now, long long *last_stats_ms, long long *stats_grab_base) {
    double elapsed = (now - (*last_stats_ms)) / 1000.0;
    long long grabs = g_grab_count - (*stats_grab_base);
    fprintf(stderr, "[evdi-helper] %.1f grabs/s (total %lld), mode:%d dpms:%d pending:%d\n",
            elapsed > 0 ? grabs / elapsed : 0,
            g_grab_count, g_have_mode, g_dpms_on, g_update_pending);
    fprintf(stderr, "[evdi-helper] cycle: request→ready avg %.1fms (%d waited, %d immediate), grab avg %.1fms (%d, %d empty)\n",
            g_wait_n ? g_wait_sum_us / 1000.0 / g_wait_n : 0.0, g_wait_n, g_immediate_n,
            g_grab_n ? g_grab_sum_us / 1000.0 / g_grab_n : 0.0, g_grab_n, g_empty_n);
    g_wait_sum_us = 0; g_wait_n = 0; g_grab_sum_us = 0; g_grab_n = 0;
    g_immediate_n = 0; g_empty_n = 0;

    /* Capture-side latency: grab → convert → into the encoder's FIFO. */
    pthread_mutex_lock(&g_swap_mutex);
    int n = g_lat_count;
    int snapshot[LAT_SAMPLES];
    if (n > 0) memcpy(snapshot, g_lat_us, (size_t)n * sizeof(int));
    g_lat_count = 0;
    pthread_mutex_unlock(&g_swap_mutex);
    if (n > 0) {
        qsort(snapshot, (size_t)n, sizeof(int), cmp_int);
        fprintf(stderr,
                "[evdi-helper] capture→fifo p50 %.1fms p95 %.1fms (%d frames)\n",
                snapshot[n / 2] / 1000.0,
                snapshot[(int)((n - 1) * 0.95)] / 1000.0, n);
    }

    (*stats_grab_base) = g_grab_count;
    (*last_stats_ms) = now;
}

/* -1: channel failed, 0: interrupted, 1: events handled or timeout elapsed. */
static int poll_capture_events(evdi_handle handle, struct evdi_event_context *evtctx,
                               struct pollfd *fd, int timeout_ms) {
    int ret = poll(fd, 1, timeout_ms);
    if (ret < 0) {
        if (errno == EINTR) return 0;
        fprintf(stderr, "[evdi-helper] poll() error: %s\n", strerror(errno));
        return -1;
    }
    if (fd->revents & (POLLHUP | POLLERR | POLLNVAL)) {
        fprintf(stderr, "[evdi-helper] Event channel failed (poll flags 0x%x)\n", fd->revents);
        return -1;
    }
    if (ret > 0 && (fd->revents & POLLIN)) {
        /* update_ready / mode_changed handlers fire from here */
        evdi_handle_events(handle, evtctx);
    }
    return 1;
}

static void run_event_loop(evdi_handle handle) {
    struct evdi_event_context evtctx = {
        .dpms_handler = on_dpms,
        .mode_changed_handler = on_mode_changed,
        .update_ready_handler = on_update_ready,
        .crtc_state_handler = on_crtc_state,
        .cursor_set_handler = on_cursor_set,
        .cursor_move_handler = on_cursor_move,
        .user_data = NULL,
    };

    struct pollfd fds[1];
    fds[0].fd = evdi_get_event_ready(handle);
    fds[0].events = POLLIN;

    long long last_stats_ms = now_ms();
    long long last_fallback_grab_ms = 0;
    if (getenv("USCREEN_NO_PIPELINE")) g_pipeline = 0;
    long long stats_grab_base = 0;
    long request_period_ms = capture_period_ms();

    while (g_running) {
        int timeout_ms = capture_poll_timeout(request_period_ms);

        int ret = poll_capture_events(handle, &evtctx, fds, timeout_ms);
        if (ret < 0) break;
        if (ret == 0) continue;

        if (!g_have_mode)
            continue;

        long long now = now_ms();

        request_capture_if_due(handle, now, request_period_ms);

        recover_capture_if_stalled(now, &last_fallback_grab_ms);

        if (now - last_stats_ms >= 5000) {
            report_capture_stats(now, &last_stats_ms, &stats_grab_base);
        }
    }
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
        g_scale = atoi(value);
        if (g_scale < 1) g_scale = 1;
        if (g_scale > 4) g_scale = 4;
    } else if (strcmp(name, "--card") == 0) {
        /* Pin the assigned card; never borrow another tablet's slot. */
        g_pin_card = atoi(value);
    } else if (strcmp(name, "--fps") == 0) {
        g_fps = atoi(value);
        if (g_fps < 1 || g_fps > 240) g_fps = 60;
    } else {
        return 0;
    }
    return 1;
}

static int set_helper_option(helper_options_t *options, const char *name, const char *value) {
    if (strcmp(name, "--edid") == 0) options->edid_path = value;
    else if (strcmp(name, "--capture-fifo") == 0) options->fifo_path = value;
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
    {
        pthread_condattr_t ca;
        pthread_condattr_init(&ca);
        pthread_condattr_setclock(&ca, CLOCK_MONOTONIC);
        pthread_cond_init(&g_frame_ready, &ca);
        pthread_condattr_destroy(&ca);
    }

    struct sigaction sa;
    memset(&sa, 0, sizeof(sa));
    sa.sa_handler = handle_signal;
    sigaction(SIGINT, &sa, NULL);
    sigaction(SIGTERM, &sa, NULL);
    signal(SIGPIPE, SIG_IGN);

}

static evdi_handle acquire_capture_device(int *index) {
    /* Reuse an existing EVDI device if one is free (e.g. from a previous
       run) — adding a new DRM card on every restart floods the compositor
       with display hotplug events. */
    evdi_handle handle = open_available_device_in("/sys/devices/platform", g_pin_card, index);
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
        handle = wait_for_available_device("/sys/devices/platform", 5000, index);
        if (handle == EVDI_INVALID_HANDLE) {
            fprintf(stderr, "[evdi-helper] No free EVDI device appeared within timeout.\n"
                            "[evdi-helper] Either the evdi kernel module is not loaded, or no device exists\n"
                            "[evdi-helper] and /sys/devices/evdi/add is root-only. Check `lsmod | grep evdi`;\n"
                            "[evdi-helper] then, once: echo 'options evdi initial_device_count=2' | sudo tee /etc/modprobe.d/uscreen-evdi.conf\n"
                            "[evdi-helper]            sudo modprobe -r evdi; sudo modprobe evdi   (or reboot)\n"
                            "[evdi-helper] The packages and install.sh do this — unless evdi-dkms failed to build, see docs/installation.md.\n");
            return EVDI_INVALID_HANDLE;
        }
        fprintf(stderr, "[evdi-helper] Found EVDI device at /dev/dri/card%d\n", (*index));
    }
    return handle;
}

static unsigned char *read_edid_file(const char *edid_path, long *size) {
    FILE *f = fopen(edid_path, "rb");
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
        g_fifo_path = fifo_path;
        conv_pool_init();   /* spawn NV12 conversion workers before first grab */
        fprintf(stderr, "[evdi-helper] Capture FIFO: %s (opened on demand)\n", fifo_path);
        if (pthread_create(writer, NULL, writer_thread, NULL) != 0) {
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
    pthread_mutex_lock(&g_swap_mutex);
    pthread_cond_broadcast(&g_frame_ready);
    pthread_mutex_unlock(&g_swap_mutex);
    if (writer) pthread_join(writer, NULL);
    if (g_capture_fifo_fd >= 0) close(g_capture_fifo_fd);
    free(g_framebuffer);
    free(g_fill);
    free(g_latest);
    free(g_write);
    free(g_dirty_fill);
    free(g_dirty_latest);
    free(g_dirty_write);

    fprintf(stderr, "[evdi-helper] Disconnecting...\n");
    evdi_disconnect(handle);
    evdi_close(handle);
    g_handle = EVDI_INVALID_HANDLE;

    fprintf(stderr, "[evdi-helper] Done.\n");
}

int main(int argc, char *argv[]) {
    helper_options_t options = parse_helper_options(argc, argv);
    const char *edid_path = options.edid_path;
    const char *fifo_path = options.fifo_path;

    if (!edid_path) {
        fprintf(stderr, "Usage: %s --edid <edid.bin> [--capture-fifo <path>] [--fps <n>] [--scale <1-4>]\n", argv[0]);
        return 1;
    }

    initialize_helper_runtime();

    int dev_idx = -1;
    evdi_handle handle = acquire_capture_device(&dev_idx);
    if (handle == EVDI_INVALID_HANDLE) return 1;
    g_device_index = dev_idx;
    g_handle = handle;

    long edid_size;
    unsigned char *edid = read_edid_file(edid_path, &edid_size);
    if (!edid) {
        evdi_close(handle);
        return 1;
    }

    fprintf(stderr, "[evdi-helper] Connecting with EDID (%ld bytes)...\n", edid_size);
    evdi_connect(handle, edid, (unsigned int)edid_size, 0);
    free(edid);

    printf("EVDI_CONNECTED card%d\n", dev_idx);
    fflush(stdout);

    pthread_t writer = 0;
    if (!start_capture_writer(fifo_path, &writer)) return 1;

    fprintf(stderr, "[evdi-helper] Connected. Capture at %d fps. Entering event loop.\n", g_fps);
    run_event_loop(handle);

    shutdown_capture(handle, writer);
    return 0;
}
