#define _POSIX_C_SOURCE 200809L
#include "gpu.h"
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
#include <unistd.h>

static void pace(uint64_t deadline) {
    struct timespec when = {deadline / 1000000000, deadline % 1000000000};
    while (clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &when, NULL) == EINTR) {}
}

static void stream(gpu_capture *capture, gpu_encoder *encoder, const gpu_options *options) {
    uint64_t deadline = gpu_now_ns(), period = 1000000000 / options->fps;
    uint64_t previous = 0;
    int trace = getenv("BLENT_GPU_TRACE") != NULL;
    for (unsigned sequence = 0; !options->limit || sequence < options->limit; sequence++) {
        alarm(0);
        if (capture->damage) { alarm(2); gpu_events_wait(capture, previous, options->fps); }
        else pace(deadline);
        /* Native X/VA waits and backpressured stdout cannot stall indefinitely.
         * The supervisor observes exit and resumes the ordinary FIFO adapter. */
        alarm(2);
        uint64_t start = gpu_now_ns();
        previous = start;
        gpu_capture_take(capture, options, sequence);
        gpu_encode(encoder, capture, options, start / 1000);
        if (trace) fprintf(stderr, "[gpu-frame] %u %llu %llu\n", sequence,
            (unsigned long long)start, (unsigned long long)gpu_now_ns());
        deadline += period;
        if (deadline < start) deadline = start + period;
    }
}

int main(int argc, char **argv) {
    gpu_options options = gpu_parse(argc, argv);
    gpu_capture capture = {0}; gpu_encoder encoder = {0};
    alarm(5);
    gpu_capture_open(&capture, &options);
    gpu_encoder_open(&encoder, &capture, &options);
    gpu_emit_header(&options);
    stream(&capture, &encoder, &options);
    gpu_encoder_close(&encoder); gpu_capture_close(&capture);
    alarm(0);
    return 0;
}
