#ifndef BLENT_REPLAY_EXCHANGE_H
#define BLENT_REPLAY_EXCHANGE_H
#include "frame_exchange.h"
#include <assert.h>

/* Adapt only the harness; production files retain exact revision bytes. */
static inline void replay_publish(frame_exchange_t *frames) {
#ifdef BLENT_EXCHANGE_GENERATION
    unsigned generation;
    assert(frame_exchange_begin(frames, &generation));
    frame_exchange_publish(frames, 0, generation);
#else
    frame_exchange_publish(frames, 0);
#endif
}
#endif
