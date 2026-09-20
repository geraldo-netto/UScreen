#ifndef USCREEN_WRITER_H
#define USCREEN_WRITER_H
#include "frame_exchange.h"
#include "fifo_writer.h"

/* Immutable arguments owned by the caller until writer_run returns/join.
 * Only this thread touches FIFO state; frame leases remain held across pacing
 * and all partial writes, and are released before mode buffers can retire. */
typedef struct {
    frame_exchange_t *frames;
    fifo_writer_t *fifo;
    atomic_int *running;
    int fps;
    const char *idle_control;
} writer_context_t;
void *writer_run(void *context);
#endif
