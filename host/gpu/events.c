#define _POSIX_C_SOURCE 200809L
#include "gpu.h"
#include <errno.h>
#include <poll.h>
#include <stdlib.h>
#include <string.h>

void gpu_events_open(gpu_capture *c, const gpu_options *options) {
    const char *mode = getenv("BLENT_GPU_CADENCE");
    if (!mode || !strcmp(mode, "periodic") || options->pattern) return;
    gpu_require(!strcmp(mode, "damage"), "cadence must be periodic or damage");
    int error, major, minor;
    gpu_require(XDamageQueryExtension(c->display, &c->damage_event, &error), "XDamage extension");
    gpu_require(XFixesQueryExtension(c->display, &c->cursor_event, &error), "XFixes extension");
    gpu_require(XDamageQueryVersion(c->display, &major, &minor), "XDamage version");
    c->damage = XDamageCreate(c->display, c->root, XDamageReportNonEmpty);
    c->region = XFixesCreateRegion(c->display, NULL, 0);
    XFixesSelectCursorInput(c->display, c->root, XFixesDisplayCursorNotifyMask);
    c->dirty = 1;
    XFlush(c->display);
}

static int damaged(gpu_capture *c) {
    XDamageSubtract(c->display, c->damage, None, c->region);
    int count = 0, changed = 0;
    XRectangle *rectangles = XFixesFetchRegion(c->display, c->region, &count);
    gpu_require(count == 0 || rectangles != NULL, "damage rectangles");
    for (int i = 0; i < count; i++) {
        XRectangle r = rectangles[i];
        changed |= gpu_intersects(c->x, c->y, c->width, c->height, r.x, r.y, r.width, r.height);
    }
    XFree(rectangles);
    return changed;
}

static void events(gpu_capture *c) {
    /* Bound queue work even while other desktop clients produce events. */
    for (unsigned i = 0; i < 256 && XPending(c->display); i++) {
        XEvent event; XNextEvent(c->display, &event);
        if (event.type == c->damage_event + XDamageNotify) c->dirty |= damaged(c);
        if (event.type == c->cursor_event + XFixesCursorNotify) c->dirty = 1;
    }
}

static void sample_pointer(gpu_capture *c) {
    Window root, child;
    int x, y, local_x, local_y; unsigned mask;
    gpu_require(XQueryPointer(c->display, c->root, &root, &child, &x, &y,
                            &local_x, &local_y, &mask), "cursor position");
    /* Shape notifications do not include motion. Bounded polling also catches
     * cursor entry/exit without selecting or stealing another client's input. */
    if (x != c->pointer_x || y != c->pointer_y) c->dirty = 1;
    c->pointer_x = x; c->pointer_y = y;
}

void gpu_events_wait(gpu_capture *c, uint64_t previous, unsigned fps) {
    gpu_require(!c->leased, "event wait after final GPU consumer");
    for (;;) {
        events(c); sample_pointer(c);
        uint64_t now = gpu_now_ns();
        uint64_t deadline = gpu_capture_deadline(previous, fps, c->dirty);
        if (!previous || now >= deadline) { c->dirty = 0; return; }
        int milliseconds = (int)((deadline - now + 999999) / 1000000);
        if (milliseconds > 8) milliseconds = 8;
        struct pollfd fd = {.fd=ConnectionNumber(c->display), .events=POLLIN};
        int result = poll(&fd, 1, milliseconds);
        gpu_require(result >= 0 || errno == EINTR, "bounded X11 event wait");
        gpu_require(!(fd.revents & (POLLERR | POLLHUP | POLLNVAL)), "live X11 connection");
    }
}

void gpu_events_close(gpu_capture *c) {
    if (!c->damage) return;
    XDamageDestroy(c->display, c->damage);
    XFixesDestroyRegion(c->display, c->region);
}
