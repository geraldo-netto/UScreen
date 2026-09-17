#include "conversion.h"
#include <stdio.h>
#include <string.h>

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

static void *conv_worker(void *arg) {
    conv_worker_arg_t *worker = arg;
    conv_pool_t *pool = worker->pool;
    int id = worker->id;
    unsigned int last_gen = 0;
    for (;;) {
        pthread_mutex_lock(&pool->mutex);
        while (pool->generation == last_gen && !pool->shutdown)
            pthread_cond_wait(&pool->ready, &pool->mutex);
        if (pool->shutdown) { pthread_mutex_unlock(&pool->mutex); break; }
        last_gen = pool->generation;
        pthread_mutex_unlock(&pool->mutex);

        if (pool->jobs[id].scale > 1) convert_strip_scaled(&pool->jobs[id]);
        else convert_strip(&pool->jobs[id]);

        pthread_mutex_lock(&pool->mutex);
        if (--pool->active == 0)
            pthread_cond_signal(&pool->done);
        pthread_mutex_unlock(&pool->mutex);
    }
    return NULL;
}

void conv_pool_start(conv_pool_t *pool, int count) {
    pool->count = count < 1 ? 1 : count;
    if (pool->count > MAX_CONV_THREADS) pool->count = MAX_CONV_THREADS;
    /* Worker threads handle jobs 1..n-1; the caller runs job 0 itself. */
    for (int i = 1; i < pool->count; i++) {
        pool->workers[i] = (conv_worker_arg_t){pool, i};
        int error = pthread_create(&pool->threads[i], NULL, conv_worker, &pool->workers[i]);
        if (error != 0) {
            fprintf(stderr, "[evdi-helper] Conversion worker unavailable: %s\n", strerror(error));
            pool->count = i;
            break;
        }
    }
    fprintf(stderr, "[evdi-helper] NV12 conversion using %d thread(s)\n", pool->count);
}

void conv_pool_convert(conv_pool_t *pool, const conv_job_t *frame) {
    int rows = frame->cy1 - frame->cy0;
    for (int i = 0; i < pool->count; i++) {
        pool->jobs[i] = *frame;
        pool->jobs[i].cy0 = frame->cy0 + rows * i / pool->count;
        pool->jobs[i].cy1 = frame->cy0 + rows * (i + 1) / pool->count;
    }
    if (pool->count > 1) {
        pthread_mutex_lock(&pool->mutex);
        pool->active = pool->count - 1;
        pool->generation++;
        pthread_cond_broadcast(&pool->ready);
        pthread_mutex_unlock(&pool->mutex);
    }
    if (pool->jobs[0].scale > 1) convert_strip_scaled(&pool->jobs[0]);
    else convert_strip(&pool->jobs[0]);   /* caller does its own strip */
    if (pool->count > 1) {
        pthread_mutex_lock(&pool->mutex);
        while (pool->active > 0)
            pthread_cond_wait(&pool->done, &pool->mutex);
        pthread_mutex_unlock(&pool->mutex);
    }
}

void conv_pool_stop(conv_pool_t *pool) {
    pthread_mutex_lock(&pool->mutex);
    pool->shutdown = 1;
    pthread_cond_broadcast(&pool->ready);
    pthread_mutex_unlock(&pool->mutex);
    for (int i = 1; i < pool->count; i++) pthread_join(pool->threads[i], NULL);
}

void conv_pool_destroy(conv_pool_t *pool) {
    conv_pool_stop(pool);
    pthread_cond_destroy(&pool->ready);
    pthread_cond_destroy(&pool->done);
    pthread_mutex_destroy(&pool->mutex);
}
