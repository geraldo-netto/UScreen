#ifndef USCREEN_PIXEL_DAMAGE_H
#define USCREEN_PIXEL_DAMAGE_H
#include "pixel_span.h"
#include <stdint.h>
#include <string.h>

/* CPU-owned histories. Updating metadata never grants access to leased pixels.
 * Dimensions and backing storage are validated by the buffer owner. */
/* OR one bit range without revisiting every row of overlapping rectangles. */
static inline void pixel_damage_range(unsigned char *mask, int first, int end) {
    if (first >= end) return;
    int begin_byte = first / 8, last_byte = (end - 1) / 8;
    unsigned char left = (unsigned char)(0xFFu << (first & 7));
    unsigned char right = (unsigned char)(0xFFu >> (7 - ((end - 1) & 7)));
    if (begin_byte == last_byte) { mask[begin_byte] |= left & right; return; }
    mask[begin_byte] |= left;
    memset(mask + begin_byte + 1, 0xFF, (size_t)(last_byte - begin_byte - 1));
    mask[last_byte] |= right;
}

/* Normalize in source space before rounding. Wider arithmetic also handles
 * INT_MIN/INT_MAX damage reported outside a validated framebuffer. */
static inline pixel_span_t pixel_chroma_range(int begin, int end, int scale, int limit) {
    if (end < begin) { int swap = begin; begin = end; end = swap; }
    if (begin == end) return (pixel_span_t){0};
    int64_t first = begin, last = end, divisor = 2 * scale;
    if (first < 0) first = 0;
    if (last > (int64_t)limit * divisor) last = (int64_t)limit * divisor;
    if (first >= last) return (pixel_span_t){0};
    return (pixel_span_t){(int)(first / divisor), (int)((last + divisor - 1) / divisor)};
}

static inline void pixel_damage_merge(unsigned char *mask, pixel_span_t *spans, pixel_span_t rows, pixel_span_t x) {
    if (!spans) return;
    for (int cy = rows.begin; cy < rows.end; cy++) {
        if (!(mask[cy / 8] & (1u << (cy % 8)))) spans[cy] = x;
        if (x.begin < spans[cy].begin) spans[cy].begin = x.begin;
        if (x.end > spans[cy].end) spans[cy].end = x.end;
    }
}

static inline void pixel_damage_region(unsigned char *mask, pixel_span_t *spans,
                                        pixel_span_t rows, pixel_span_t x) {
    pixel_damage_merge(mask, spans, rows, x);
    pixel_damage_range(mask, rows.begin, rows.end);
}
#endif
