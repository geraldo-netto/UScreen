#include <stddef.h>
#include <stdint.h>

/* T419 verification only; this is not a cryptographic wire-integrity mechanism. */
uint64_t rect_rgb_hash(const unsigned char *data, size_t pixels, size_t stride) {
    uint64_t hash = UINT64_C(14695981039346656037);
    for (size_t pixel = 0; pixel < pixels; pixel++) {
        for (size_t channel = 0; channel < 3; channel++) {
            hash ^= data[pixel * stride + channel];
            hash *= UINT64_C(1099511628211);
        }
    }
    return hash;
}
