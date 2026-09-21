#ifndef USCREEN_RAW_RING_H
#define USCREEN_RAW_RING_H
#include <stdint.h>
#include <stddef.h>
#include <stdatomic.h>

#define RAW_MESSAGE_BYTES 96
#define RAW_CONTROL_BYTES 4096
#define RAW_SLOT_CONTROL_BYTES 32
#define RAW_SLOTS 4
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
} raw_ring_t;
#define RAW_RING_INITIALIZER { .socket = -1, .fd = -1, .slots = RAW_SLOTS }

int raw_ring_init(raw_ring_t *ring, int socket);
int raw_ring_resize(raw_ring_t *ring, uint32_t width, uint32_t height);
/* Service at most 32 control messages per capture iteration. */
int raw_ring_service(raw_ring_t *ring);
unsigned char *raw_ring_acquire(raw_ring_t *ring, uint32_t *slot);
int raw_ring_publish(raw_ring_t *ring, uint32_t slot, uint64_t captured_us);
void raw_ring_close(raw_ring_t *ring);
#endif
