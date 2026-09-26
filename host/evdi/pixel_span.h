#ifndef BLENT_PIXEL_SPAN_H
#define BLENT_PIXEL_SPAN_H
/* Half-open pixel interval. Conversion spans use even output coordinates:
 * each interval contains complete 2x2 NV12 chroma blocks. No native OS types. */
typedef struct { int begin, end; } pixel_span_t;
#endif
