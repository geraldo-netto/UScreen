#ifndef BLENT_CONVERSION_H
#define BLENT_CONVERSION_H
#include <pthread.h>
#include "pixel_span.h"

typedef struct {
    const unsigned char *src;   /* BGRA, stride-padded */
    unsigned char *ydst;        /* Y plane, w bytes/row */
    unsigned char *uvdst;       /* interleaved CbCr, w bytes per chroma row */
    int w, h, stride;           /* source dimensions */
    int ow, oh;                 /* destination dimensions (w/scale, h/scale) */
    int scale;                  /* 1 = no downscale */
    int cy0, cy1;               /* chroma-row range [cy0, cy1) this job owns */
    const unsigned char *dirty; /* NULL = all rows; otherwise one bit/chroma row */
    const pixel_span_t *spans;  /* Optional per-chroma-row bounds, NULL = full width.
                                * Borrowed, aligned and clipped by frame owner. */
} conv_job_t;

#define MAX_CONV_THREADS 128
typedef struct conv_pool conv_pool_t;
typedef struct {
    conv_pool_t *pool;
    int id, pending;
    pthread_cond_t ready;
} conv_worker_arg_t;
struct conv_pool {
    int count, last_jobs;
    pthread_t threads[MAX_CONV_THREADS];
    conv_job_t jobs[MAX_CONV_THREADS];
    conv_worker_arg_t workers[MAX_CONV_THREADS];
    pthread_mutex_t mutex;
    pthread_cond_t done;
    unsigned generation;
    int active, shutdown;
};
#define CONV_POOL_INITIALIZER { .count = 1, .last_jobs = 1, .mutex = PTHREAD_MUTEX_INITIALIZER, \
    .done = PTHREAD_COND_INITIALIZER }

/* Initialize with CONV_POOL_INITIALIZER; do not move after start. One capture
 * thread owns submission. convert() joins every job before returning, so input,
 * output and dirty masks are borrowed only for the duration of that call.
 * Native/scaled kernels retain distinct hot loops. Destroy after final convert. */
void conv_pool_start(conv_pool_t *pool, int count);
void conv_pool_convert(conv_pool_t *pool, const conv_job_t *frame);
void conv_pool_stop(conv_pool_t *pool);
void conv_pool_destroy(conv_pool_t *pool);
#endif
