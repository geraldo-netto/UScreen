#define _GNU_SOURCE
#include "raw_ring.h"
#include "pixel_damage.h"
#include <stdlib.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <sys/stat.h>
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#include <string.h>
#include <endian.h>

_Static_assert(ATOMIC_INT_LOCK_FREE == 2, "raw slot state must be lock-free");
_Static_assert(ATOMIC_LLONG_LOCK_FREE == 2, "raw slot sequence must be lock-free");

static void raw_u32(unsigned char *message, int offset, uint32_t value) {
    value = htole32(value); memcpy(message + offset, &value, 4);
}
static void raw_u64(unsigned char *message, int offset, uint64_t value) {
    value = htole64(value); memcpy(message + offset, &value, 8);
}
static uint64_t raw_read64(const unsigned char *message, int offset) {
    uint64_t value; memcpy(&value, message + offset, 8); return le64toh(value);
}
static uint32_t raw_read32(const unsigned char *message, int offset) {
    uint32_t value; memcpy(&value, message + offset, 4); return le32toh(value);
}

static void raw_message(const raw_ring_t *ring, unsigned char *message, uint32_t kind,
                        uint32_t slot, uint64_t captured_us) {
    memset(message, 0, RAW_MESSAGE_BYTES);
    raw_u32(message, 0, 0x52435355); raw_u32(message, 4, 1);
    raw_u32(message, 8, kind); raw_u32(message, 12, 0x3231564e);
    raw_u64(message, 16, ring->generation); raw_u64(message, 24, ring->nonce);
    raw_u32(message, 32, ring->width); raw_u32(message, 36, ring->height);
    raw_u32(message, 40, ring->width); raw_u32(message, 44, ring->width);
    raw_u32(message, 48, ring->width * ring->height);
    raw_u32(message, 52, ring->slot_bytes); raw_u32(message, 56, ring->slots);
    raw_u32(message, 60, RAW_CONTROL_BYTES); raw_u64(message, 64, ring->bytes);
    raw_u64(message, 72, ring->sequence); raw_u32(message, 80, slot);
    raw_u64(message, 88, captured_us);
}

static int raw_send(raw_ring_t *ring, uint32_t kind, uint32_t slot, uint64_t captured_us) {
    unsigned char bytes[RAW_MESSAGE_BYTES];
    raw_message(ring, bytes, kind, slot, captured_us);
    struct iovec vector = {bytes, sizeof(bytes)};
    union { struct cmsghdr aligned; char bytes[CMSG_SPACE(sizeof(int))]; } control = {0};
    struct msghdr message = {.msg_iov = &vector, .msg_iovlen = 1};
    if (kind == 1) {
        message.msg_control = control.bytes; message.msg_controllen = sizeof(control);
        struct cmsghdr *header = CMSG_FIRSTHDR(&message);
        header->cmsg_level = SOL_SOCKET; header->cmsg_type = SCM_RIGHTS;
        header->cmsg_len = CMSG_LEN(sizeof(int));
        memcpy(CMSG_DATA(header), &ring->fd, sizeof(int));
    }
    ssize_t result = sendmsg(ring->socket, &message, MSG_NOSIGNAL | MSG_DONTWAIT);
    if (result == RAW_MESSAGE_BYTES) return 1;
    if (result < 0 && (errno == EAGAIN || errno == EINTR)) return 0;
    return -1;
}

static void raw_retire(raw_ring_t *ring) {
    for (unsigned slot = 0; slot < RAW_MAX_SLOTS; slot++) {
        free(ring->dirty[slot]); ring->dirty[slot] = NULL;
        free(ring->spans[slot]); ring->spans[slot] = NULL;
    }
    if (ring->memory) munmap(ring->memory, ring->bytes);
    if (ring->fd >= 0) close(ring->fd);
    ring->memory = NULL; ring->fd = -1;
}

int raw_ring_init(raw_ring_t *ring, int socket) {
    int type = 0; socklen_t size = sizeof(type);
    if (getsockopt(socket, SOL_SOCKET, SO_TYPE, &type, &size) < 0 || type != SOCK_SEQPACKET) return 0;
    struct sockaddr_un peer;
    socklen_t peer_size = sizeof(peer);
    if (getpeername(socket, (struct sockaddr *)&peer, &peer_size) < 0 || peer.sun_family != AF_UNIX) return 0;
    int flags = fcntl(socket, F_GETFL);
    if (flags < 0 || fcntl(socket, F_SETFL, flags | O_NONBLOCK) < 0) return 0;
    ring->socket = socket;
    return 1;
}

static int raw_geometry(raw_ring_t *ring, uint32_t width, uint32_t height) {
    if (ring->slots < 2 || ring->slots > RAW_MAX_SLOTS) return 0;
    if (width < 2 || width > 4096 || height < 2 || height > 4096) return 0;
    if ((width | height) & 1) return 0;
    ring->width = width; ring->height = height;
    ring->slot_bytes = ((width * height * 3 / 2 + 64 + 4095) / 4096) * 4096;
    ring->bytes = RAW_CONTROL_BYTES + (size_t)ring->slot_bytes * ring->slots;
    return 1;
}

void raw_ring_mark_all(raw_ring_t *ring) {
    for (unsigned slot = 0; slot < ring->slots; slot++) {
        if (!ring->dirty[slot]) continue;
        pixel_damage_region(ring->dirty[slot], ring->spans[slot],
            (pixel_span_t){0, ring->height / 2}, (pixel_span_t){0, ring->width});
    }
}

void raw_ring_damage(raw_ring_t *ring, int x0, int y0, int x1, int y1, int scale) {
    if (scale < 1 || scale > 4) return;
    pixel_span_t x = pixel_chroma_range(x0, x1, scale, ring->width / 2);
    if (x.begin == x.end) return;
    pixel_span_t rows = pixel_chroma_range(y0, y1, scale, ring->height / 2);
    for (unsigned slot = 0; slot < ring->slots; slot++) {
        if (!ring->dirty[slot]) continue;
        pixel_damage_region(ring->dirty[slot], ring->spans[slot], rows,
            (pixel_span_t){2 * x.begin, 2 * x.end});
    }
}

void raw_ring_converted(raw_ring_t *ring, uint32_t slot) {
    if (slot >= ring->slots || !ring->dirty[slot]) return;
    memset(ring->dirty[slot], 0, (ring->height / 2 + 7) / 8);
}

static int raw_allocate_histories(raw_ring_t *ring) {
    for (unsigned slot = 0; slot < ring->slots; slot++) {
        ring->dirty[slot] = malloc((ring->height / 2 + 7) / 8);
        ring->spans[slot] = malloc(ring->height / 2 * sizeof(pixel_span_t));
        if (!ring->dirty[slot] || !ring->spans[slot]) return 0;
        raw_ring_converted(ring, slot);
    }
    raw_ring_mark_all(ring);
    return 1;
}

static int raw_allocate(raw_ring_t *ring) {
    if (!raw_allocate_histories(ring)) return 0;
    ring->fd = memfd_create("uscreen-raw", MFD_CLOEXEC | MFD_ALLOW_SEALING);
    if (ring->fd < 0) return 0;
    if (ftruncate(ring->fd, (off_t)ring->bytes) < 0) return 0;
    int seals = F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL;
    if (fcntl(ring->fd, F_ADD_SEALS, seals) < 0) return 0;
    void *memory = mmap(NULL, ring->bytes, PROT_READ | PROT_WRITE, MAP_SHARED, ring->fd, 0);
    if (memory == MAP_FAILED) return 0;
    ring->memory = memory;
    for (uint32_t slot = 0; slot < ring->slots; slot++) {
        atomic_init((_Atomic uint32_t *)(ring->memory + slot * RAW_SLOT_CONTROL_BYTES), RAW_FREE);
        atomic_init((_Atomic uint64_t *)(ring->memory + slot * RAW_SLOT_CONTROL_BYTES + 8), 0);
    }
    return 1;
}

int raw_ring_resize(raw_ring_t *ring, uint32_t width, uint32_t height) {
    raw_ring_t next = RAW_RING_INITIALIZER;
    next.slots = ring->slots;
    if (!raw_geometry(&next, width, height)) return 0;
    next.socket = ring->socket; next.nonce = ring->nonce;
    next.generation = ring->generation + 1;
    if (!next.generation) return 0;
    if (next.nonce && (!raw_allocate(&next) || raw_send(&next, 1, 0, 0) != 1)) {
        raw_retire(&next); return 0;
    }
    raw_retire(ring);
    *ring = next;
    return 1;
}

/* Always close received rights, even for malformed packets. */
static int raw_close_rights(struct msghdr *message) {
    int count = 0;
    for (struct cmsghdr *header = CMSG_FIRSTHDR(message); header;
            header = CMSG_NXTHDR(message, header)) {
        if (header->cmsg_level != SOL_SOCKET || header->cmsg_type != SCM_RIGHTS) continue;
        size_t bytes = header->cmsg_len - CMSG_LEN(0);
        for (size_t offset = 0; offset + sizeof(int) <= bytes; offset += sizeof(int)) {
            int fd; memcpy(&fd, CMSG_DATA(header) + offset, sizeof(fd)); close(fd); count++;
        }
    }
    return count;
}

static int raw_control(raw_ring_t *ring, const unsigned char *bytes) {
    raw_ring_t expected = RAW_RING_INITIALIZER;
    expected.slots = raw_read32(bytes, 56);
    if (!raw_geometry(&expected, raw_read32(bytes, 32), raw_read32(bytes, 36))) return 0;
    expected.generation = raw_read64(bytes, 16); expected.nonce = raw_read64(bytes, 24);
    expected.sequence = raw_read64(bytes, 72);
    uint32_t kind = raw_read32(bytes, 8), slot = raw_read32(bytes, 80);
    if (!expected.generation || !expected.nonce || slot >= expected.slots) return 0;
    unsigned char canonical[RAW_MESSAGE_BYTES];
    raw_message(&expected, canonical, kind, slot, raw_read64(bytes, 88));
    if (memcmp(bytes, canonical, sizeof(canonical)) != 0) return 0;
    if (kind == 3) return 1; /* Release notifications are hints; atomics own state. */
    if (kind != 4) return 0;
    ring->nonce = expected.nonce;
    ring->slots = expected.slots;
    if (!ring->width) return 1;
    return raw_ring_resize(ring, ring->width, ring->height);
}

static int raw_receive(raw_ring_t *ring) {
    unsigned char bytes[RAW_MESSAGE_BYTES];
    struct iovec vector = {bytes, sizeof(bytes)};
    union { struct cmsghdr aligned; char bytes[CMSG_SPACE(8 * sizeof(int))]; } control = {0};
    struct msghdr message = {.msg_iov = &vector, .msg_iovlen = 1,
        .msg_control = control.bytes, .msg_controllen = sizeof(control)};
    ssize_t count = recvmsg(ring->socket, &message, MSG_CMSG_CLOEXEC | MSG_DONTWAIT);
    if (count < 0 && (errno == EAGAIN || errno == EINTR)) return 0;
    int rights = raw_close_rights(&message);
    if (count != RAW_MESSAGE_BYTES || rights || message.msg_flags & (MSG_TRUNC | MSG_CTRUNC)) return -1;
    return raw_control(ring, bytes) ? 1 : -1;
}

int raw_ring_service(raw_ring_t *ring) {
    for (int count = 0; count < 32; count++) {
        int result = raw_receive(ring);
        if (result < 0) return 0;
        if (result == 0) return 1;
    }
    return 1;
}

unsigned char *raw_ring_acquire(raw_ring_t *ring, uint32_t *slot) {
    if (!ring->memory) return NULL;
    for (*slot = 0; *slot < ring->slots; (*slot)++) {
        _Atomic uint32_t *state = (_Atomic uint32_t *)(ring->memory + *slot * RAW_SLOT_CONTROL_BYTES);
        uint32_t expected = RAW_FREE;
        if (atomic_compare_exchange_strong_explicit(state, &expected, RAW_WRITING,
                memory_order_acquire, memory_order_relaxed))
            return ring->memory + RAW_CONTROL_BYTES + *slot * ring->slot_bytes;
    }
    return NULL;
}

int raw_ring_publish(raw_ring_t *ring, uint32_t slot, uint64_t captured_us) {
    if (!ring->memory || slot >= ring->slots || ring->sequence == UINT64_MAX) return -1;
    _Atomic uint32_t *state = (_Atomic uint32_t *)(ring->memory + slot * RAW_SLOT_CONTROL_BYTES);
    if (atomic_load_explicit(state, memory_order_relaxed) != RAW_WRITING) return -1;
    _Atomic uint64_t *sequence = (_Atomic uint64_t *)(ring->memory + slot * RAW_SLOT_CONTROL_BYTES + 8);
    atomic_store_explicit(sequence, ++ring->sequence, memory_order_relaxed);
    atomic_store_explicit(state, RAW_READY, memory_order_release);
    int result = raw_send(ring, 2, slot, captured_us);
    if (result != 1) atomic_store_explicit(state, RAW_FREE, memory_order_release);
    return result;
}

void raw_ring_close(raw_ring_t *ring) {
    raw_retire(ring);
    if (ring->socket >= 0) close(ring->socket);
    ring->socket = -1;
}
