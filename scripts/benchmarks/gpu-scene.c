/* T575: timestamped X11 scene with a per-frame barcode. Own window only;
 * no input focus, pointer movement, EVDI ownership or device configuration. */
#define _POSIX_C_SOURCE 200809L
#include <X11/Xlib.h>
#include <assert.h>
#include <errno.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

static uint64_t now_ns(void) {
    struct timespec now; assert(clock_gettime(CLOCK_MONOTONIC, &now) == 0);
    return (uint64_t)now.tv_sec * 1000000000 + now.tv_nsec;
}

static void paint(Display *display, Window window, GC gc, unsigned sequence) {
    XSetForeground(display, gc, 0x55aa33);
    XFillRectangle(display, window, gc, 0, 0, 1280, 800);
    XSetForeground(display, gc, 0xdd3355);
    XFillRectangle(display, window, gc, sequence * 7 % 960, 100, 320, 300);
    for (unsigned bit = 0; bit < 24; bit++) {
        XSetForeground(display, gc, sequence & (1u << bit) ? 0xffffff : 0);
        XFillRectangle(display, window, gc, bit * 16, 0, 16, 16);
    }
}

int main(int argc, char **argv) {
    assert(argc == 4);
    int x = atoi(argv[1]), y = atoi(argv[2]), fps = atoi(argv[3]);
    assert(x >= 0 && x <= 16000);
    assert(y >= 0 && y <= 16000);
    assert(fps >= 1 && fps <= 60);
    Display *display = XOpenDisplay(NULL); assert(display);
    Window root = DefaultRootWindow(display);
    XSetWindowAttributes attributes = {.override_redirect=True, .background_pixel=0};
    Window window = XCreateWindow(display, root, x, y, 1280, 800, 0, CopyFromParent,
        InputOutput, CopyFromParent, CWOverrideRedirect | CWBackPixel, &attributes);
    GC gc = XCreateGC(display, window, 0, NULL);
    Pixmap back = XCreatePixmap(display, window, 1280, 800, DefaultDepth(display, DefaultScreen(display)));
    XMapRaised(display, window); XSync(display, False);
    uint64_t deadline = now_ns(), period = 1000000000 / fps;
    for (unsigned sequence = 1; sequence < 1000000; sequence++) {
        struct timespec when = {deadline / 1000000000, deadline % 1000000000};
        while (clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &when, NULL) == EINTR) {}
        paint(display, back, gc, sequence); XSync(display, False);
        uint64_t submitted = now_ns();
        XCopyArea(display, back, window, gc, 0, 0, 1280, 800, 0, 0); XSync(display, False);
        printf("%u %" PRIu64 " %" PRIu64 "\n", sequence, submitted, now_ns()); fflush(stdout);
        deadline += period;
        if (deadline < submitted) deadline = submitted + period;
    }
    XFreePixmap(display, back); XFreeGC(display, gc); XDestroyWindow(display, window); XCloseDisplay(display);
    return 0;
}
