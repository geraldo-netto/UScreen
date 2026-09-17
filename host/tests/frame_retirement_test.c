/* T389: retirement waits for release notifications with one absolute deadline. */
#define _GNU_SOURCE
#include <assert.h>
#include <pthread.h>
#include <stdatomic.h>
#include <time.h>
#include <unistd.h>
static int counted_usleep(useconds_t);
static int counted_wait(pthread_cond_t *, pthread_mutex_t *, const struct timespec *);
#define usleep counted_usleep
#define pthread_cond_timedwait counted_wait
#include "../evdi/frame_exchange.c"
#undef usleep
#undef pthread_cond_timedwait

static atomic_int entered, polls, waits;
static struct timespec first_deadline;
static int counted_usleep(useconds_t micros) {
    atomic_fetch_add(&polls, 1);
    entered = 1;
    return usleep(micros);
}
static int counted_wait(pthread_cond_t *condition, pthread_mutex_t *mutex, const struct timespec *deadline) {
    if (atomic_fetch_add(&waits, 1) == 0) first_deadline = *deadline;
    assert(deadline->tv_sec == first_deadline.tv_sec && deadline->tv_nsec == first_deadline.tv_nsec &&
           "T389: spurious wakes must not extend retirement's deadline");
    entered = 1;
    return pthread_cond_timedwait(condition, mutex, deadline);
}
static void *retire(void *data) {
    assert(frame_exchange_retire(data) && "T389: release must wake retirement");
    return NULL;
}
static void delay(long nanos) {
    struct timespec time = {.tv_nsec = nanos};
    while (nanosleep(&time, &time) < 0) {}
}
static void destroy(frame_exchange_t *frames) {
    frame_exchange_free(frames);
    pthread_cond_destroy(&frames->ready);
    pthread_mutex_destroy(&frames->mutex);
}
static void notification(void) {
    frame_exchange_t frames = FRAME_EXCHANGE_INITIALIZER;
    frame_exchange_init(&frames);
    frames.writer_busy = 1;
    frames.buffers_ready = frames.latest_valid = 1;
    pthread_t worker;
    assert(pthread_create(&worker, NULL, retire, &frames) == 0);
    while (!entered) delay(100000);
    for (int i = 0; i < 3; i++) {
        pthread_mutex_lock(&frames.mutex);
        assert(!frames.buffers_ready && !frames.latest_valid && frames.generation == 1);
        pthread_cond_broadcast(&frames.ready); // Unrelated notification is not a release.
        pthread_mutex_unlock(&frames.mutex);
        delay(5000000);
    }
    assert(frames.writer_busy);
    frame_exchange_release(&frames);
    pthread_join(worker, NULL);
    assert(atomic_load(&polls) == 0 && "T389: outstanding lease must not cause periodic polling");
    assert(atomic_load(&waits) >= 1);
    destroy(&frames);
}
static void deadline_and_retention(void) {
    frame_exchange_t frames = FRAME_EXCHANGE_INITIALIZER;
    frame_exchange_init(&frames);
    frame_exchange_resize(&frames, 8, 8);
    assert(frame_exchange_allocated(&frames));
    frames.write[0] = 71;
    frames.writer_busy = 1;
    polls = waits = entered = 0;
    struct timespec start, end;
    clock_gettime(CLOCK_MONOTONIC, &start);
    assert(!frame_exchange_retire(&frames));
    clock_gettime(CLOCK_MONOTONIC, &end);
    double elapsed = end.tv_sec - start.tv_sec + (end.tv_nsec - start.tv_nsec) / 1e9;
    assert(elapsed >= 0.95 && elapsed < 1.5 && "T389: retirement must use its one-second monotonic deadline");
    assert(frames.write[0] == 71 && frames.writer_busy && "T389: timeout cannot reclaim leased storage");
    frame_exchange_release(&frames);
    assert(frame_exchange_retire(&frames));
    destroy(&frames);
}
int main(void) {
    alarm(5);
    notification();
    deadline_and_retention();
    return 0;
}
