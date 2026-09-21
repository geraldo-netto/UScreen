#include "options.h"
#include <errno.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

void gpu_require(int condition, const char *operation) {
    if (condition) return;
    fprintf(stderr, "[gpu-capture] %s failed; use EVDI/FIFO fallback\n", operation);
    exit(2);
}

uint64_t gpu_now_ns(void) {
    struct timespec now;
    gpu_require(clock_gettime(CLOCK_MONOTONIC, &now) == 0, "monotonic clock");
    return (uint64_t)now.tv_sec * 1000000000 + now.tv_nsec;
}

static unsigned number(const char *text, unsigned low, unsigned high) {
    char *end = NULL; errno = 0;
    unsigned long value = strtoul(text, &end, 10);
    gpu_require(*text && !*end && !errno && value >= low && value <= high, "bounded numeric option");
    return (unsigned)value;
}

gpu_options gpu_parse(int argc, char **argv) {
    gpu_require(argc == 11, "usage: connector render-node width height scale fps quality bitrate-kbps frames desktop|pattern");
    gpu_options options = {.connector=argv[1], .device=argv[2],
        .width=number(argv[3], 2, 4096), .height=number(argv[4], 2, 4096),
        .scale=number(argv[5], 1, 4), .fps=number(argv[6], 1, 240),
        .quality=number(argv[7], 0, 51), .bitrate=number(argv[8], 1, 1000000),
        .limit=number(argv[9], 0, 1000000)};
    gpu_require(!((options.width | options.height) & 1), "even output geometry");
    gpu_require(options.width * options.scale <= 4096 && options.height * options.scale <= 4096,
                "bounded capture geometry");
    gpu_require(!strcmp(argv[10], "pattern") || !strcmp(argv[10], "desktop"), "capture mode");
    options.pattern = !strcmp(argv[10], "pattern");
    return options;
}

int gpu_edid_valid(const unsigned char *bytes, size_t length) {
    static const unsigned char magic[] = {0, 255, 255, 255, 255, 255, 255, 0};
    if (length < 128 || length > 512 || length % 128) return 0;
    if (memcmp(bytes, magic, sizeof(magic))) return 0;
    if ((bytes[126] + 1u) * 128 != length) return 0;
    for (size_t block = 0; block < length; block += 128) {
        unsigned char sum = 0;
        for (size_t i = 0; i < 128; i++) sum += bytes[block + i];
        if (sum) return 0;
    }
    return 1;
}

int gpu_same_render_node(uint64_t producer, uint64_t consumer) {
    return producer != 0 && producer == consumer;
}

int gpu_identity_transform(const int32_t matrix[9]) {
    for (int i = 0; i < 9; i++) {
        int32_t expected = i % 4 == 0 ? 65536 : 0;
        if (matrix[i] != expected) return 0;
    }
    return 1;
}
