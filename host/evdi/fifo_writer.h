#ifndef USCREEN_FIFO_WRITER_H
#define USCREEN_FIFO_WRITER_H
#include <stddef.h>
#include <stdatomic.h>
#include <sys/types.h>
#include <sys/stat.h>

/* One writer owns descriptors and quarantine identity. The capture owner
 * publishes generation and running; their addresses outlive this context. */
typedef struct {
    const char *path;
    int fd, retired_fd, retired;
    dev_t retired_device;
    ino_t retired_inode;
    atomic_int *running;
    const atomic_uint *generation;
} fifo_writer_t;
#define FIFO_WRITER_INITIALIZER(stop, epoch) \
    { .fd = -1, .retired_fd = -1, .running = (stop), .generation = (epoch) }

/* Returns an opened nonblocking descriptor; the caller assigns it to fd. */
int fifo_writer_open(fifo_writer_t *fifo);
/* Returns bytes left. A partial frame quarantines its inode until replaced. */
size_t fifo_writer_write(fifo_writer_t *fifo, const unsigned char *data,
                         size_t size, unsigned generation);
/* After writer join; closes both active and quarantined descriptors. */
void fifo_writer_close(fifo_writer_t *fifo);
#endif
