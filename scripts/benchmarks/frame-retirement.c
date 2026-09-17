/* T389: compare the actual exchange implementation's wait path. */
#define _GNU_SOURCE
#include <assert.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
#include <unistd.h>
static atomic_int entered, polls, waits;
static int counted_usleep(useconds_t micros) {
    entered = 1; polls++;
    return usleep(micros);
}
static int counted_wait(pthread_cond_t *condition, pthread_mutex_t *mutex, const struct timespec *deadline) {
    entered = 1; waits++;
    return pthread_cond_timedwait(condition, mutex, deadline);
}
#define usleep counted_usleep
#define pthread_cond_timedwait counted_wait
#include EXCHANGE_SOURCE
#undef pthread_cond_timedwait
#undef usleep
static long long now(void) {
    struct timespec time;
    assert(clock_gettime(CLOCK_MONOTONIC, &time) == 0);
    return (long long)time.tv_sec * 1000000000LL + time.tv_nsec;
}
static void delay(int milliseconds) {
    struct timespec time = {.tv_sec = milliseconds / 1000, .tv_nsec = (milliseconds % 1000) * 1000000L};
    while (nanosleep(&time, &time) < 0) {}
}
typedef struct { frame_exchange_t frame; long long elapsed, finished; int result; } trial_t;
static void *retire(void *pointer) {
    trial_t *trial = pointer;
    long long start = now();
    trial->result = frame_exchange_retire(&trial->frame);
    trial->finished = now();
    trial->elapsed = trial->finished - start;
    return NULL;
}
int main(int argc, char **argv) {
    assert(argc == 2);
    (void)counted_usleep; (void)counted_wait;
    alarm(5);
    int milliseconds = atoi(argv[1]);
    assert(milliseconds > 0 && milliseconds < 2000);
    trial_t trial = {.frame = FRAME_EXCHANGE_INITIALIZER};
    frame_exchange_init(&trial.frame);
    trial.frame.writer_busy = 1;
    pthread_t worker;
    assert(pthread_create(&worker, NULL, retire, &trial) == 0);
    while (!entered) delay(1);
    delay(milliseconds);
    long long released = now();
    frame_exchange_release(&trial.frame);
    pthread_join(worker, NULL);
    printf("{\"hold_ms\":%d,\"result\":%d,\"elapsed_ns\":%lld,\"after_release_ns\":%lld,\"polls\":%d,\"waits\":%d}\n",
           milliseconds, trial.result, trial.elapsed, trial.finished - released, (int)polls, (int)waits);
    pthread_cond_destroy(&trial.frame.ready);
    pthread_mutex_destroy(&trial.frame.mutex);
    return 0;
}
