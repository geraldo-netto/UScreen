/* T418: real sealed producer across exec, no EVDI device or display needed. */
#define _GNU_SOURCE
#include "raw_ring.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
    assert(argc == 3 && strcmp(argv[1], "--capture-socket-fd") == 0);
    alarm(20);
    raw_ring_t ring = RAW_RING_INITIALIZER;
    assert(raw_ring_init(&ring, atoi(argv[2])));
    assert(raw_ring_resize(&ring, 64, 64));
    char line[80];
    while (fgets(line, sizeof(line), stdin)) {
        unsigned width, height, value;
        int result = -1;
        if (strcmp(line, "service\n") == 0) result = raw_ring_service(&ring);
        else if (sscanf(line, "resize %u %u", &width, &height) == 2)
            result = raw_ring_resize(&ring, width, height);
        else if (sscanf(line, "frame %u", &value) == 1) {
            uint32_t slot;
            unsigned char *pixels = raw_ring_acquire(&ring, &slot);
            result = 0;
            if (pixels) {
                memset(pixels, (unsigned char)value, ring.width * ring.height * 3 / 2);
                result = raw_ring_publish(&ring, slot, 123456);
            }
        }
        printf("%d\n", result); fflush(stdout);
    }
    raw_ring_close(&ring);
    return 0;
}
