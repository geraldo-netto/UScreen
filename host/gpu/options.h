#ifndef BLENT_GPU_OPTIONS_H
#define BLENT_GPU_OPTIONS_H
#include <stdint.h>
#include <stddef.h>
typedef struct {
    const char *connector, *device;
    unsigned width, height, scale, fps, quality, bitrate, limit;
    int pattern;
} gpu_options;
void gpu_require(int condition, const char *operation);
uint64_t gpu_now_ns(void);
gpu_options gpu_parse(int argc, char **argv);
int gpu_edid_valid(const unsigned char *bytes, size_t length);
int gpu_same_render_node(uint64_t producer, uint64_t consumer);
int gpu_identity_transform(const int32_t matrix[9]);
#endif
