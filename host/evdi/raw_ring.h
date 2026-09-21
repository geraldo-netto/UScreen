#ifndef USCREEN_RAW_RING_H
#define USCREEN_RAW_RING_H
#include <stdint.h>
#include <stddef.h>
#include <stdatomic.h>
#include "pixel_span.h"

#define RAW_MESSAGE_BYTES 96
#define RAW_CONTROL_BYTES 4096
#define RAW_SLOT_CONTROL_BYTES 32
#define RAW_SLOTS 4
#define RAW_MAX_SLOTS 8
#define RAW_FREE 0
#define RAW_WRITING 1
#define RAW_READY 2
#define RAW_READING 3

/* Linux producer adapter. Capture thread is the sole producer; codec final
 * reference callbacks release slots with process-shared lock-free atomics. */
typedef struct {
    int socket;
    int fd;
    unsigned char *memory;
    uint32_t width, height, slot_bytes, slots;
    size_t bytes;
    uint64_t generation, nonce, sequence;
    /* Producer-private damage follows each slot across final-reference release.
     * Never stored in peer-writable memory or used to authorize pixel writes. */
    unsigned char *dirty[RAW_MAX_SLOTS];
    pixel_span_t *spans[RAW_MAX_SLOTS];
} raw_ring_t;
#define RAW_RING_INITIALIZER { .socket = -1, .fd = -1, .slots = RAW_SLOTS }

int raw_ring_init(raw_ring_t *ring, int socket);
int raw_ring_resize(raw_ring_t *ring, uint32_t width, uint32_t height);
/* Service at most 32 control messages per capture iteration. */
int raw_ring_service(raw_ring_t *ring);
void raw_ring_mark_all(raw_ring_t *ring);
void raw_ring_damage(raw_ring_t *ring, int x0, int y0, int x1, int y1, int scale);
/* Only after conversion completes while the producer owns RAW_WRITING. */
void raw_ring_converted(raw_ring_t *ring, uint32_t slot);
unsigned char *raw_ring_acquire(raw_ring_t *ring, uint32_t *slot);
int raw_ring_publish(raw_ring_t *ring, uint32_t slot, uint64_t captured_us);
void raw_ring_close(raw_ring_t *ring);
#endif
