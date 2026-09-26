/* T575 optional native adapter suite. Run in an isolated Xvfb server: never
 * moves the real desktop pointer, changes a monitor, or needs a GPU. */
#include "../gpu/gpu.h"
#include <X11/Xutil.h>
#include <assert.h>
#include <string.h>
#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>

static void codec_error(void) {
    pid_t pid = fork(); assert(pid >= 0);
    if (!pid) { gpu_av(AVERROR(EINVAL), "T575 forced codec rejection"); _exit(0); }
    int status; assert(waitpid(pid, &status, 0) == pid);
    assert(WIFEXITED(status) && WEXITSTATUS(status) == 2);
    gpu_av(0, "T575 successful codec operation");
}

static void fill(gpu_capture *c) {
    XSetForeground(c->display, c->gc, 0);
    XFillRectangle(c->display, c->pixmap, c->gc, 0, 0, c->width, c->height);
}

static void verify(gpu_capture *c, int present) {
    XImage *image = XGetImage(c->display, c->pixmap, 0, 0, c->width, c->height, AllPlanes, ZPixmap);
    assert(image);
    for (int y = 0; y < c->height; y++) {
        for (int x = 0; x < c->width; x++) {
            unsigned long expected = present && x < 7 && y < 8 ? 0xff0000 : 0;
            assert((XGetPixel(image, x, y) & 0xffffff) == expected);
        }
    }
    XDestroyImage(image);
}

static Cursor cursor(Display *display, Window root) {
    char bits[8]; memset(bits, 255, sizeof(bits));
    Pixmap source = XCreateBitmapFromData(display, root, bits, 8, 8);
    XColor foreground = {.red=65535}, background = {0};
    Cursor cursor = XCreatePixmapCursor(display, source, source, &foreground, &background, 3, 2);
    XFreePixmap(display, source);
    return cursor;
}

/* T579: native event plumbing on a private X server, without GPU imports. */
static void cadence(gpu_capture *c) {
    gpu_options options = {.fps=30};
    setenv("BLENT_GPU_CADENCE", "damage", 1);
    gpu_events_open(c, &options);
    XSync(c->display, False);
    gpu_events_wait(c, 0, 30); /* Drain initial full damage and pointer state. */
    uint64_t start = gpu_now_ns();
    gpu_events_wait(c, start, 30);
    assert(gpu_now_ns() - start >= 190000000); /* Idle refresh, no busy loop. */
    GC root_gc = XCreateGC(c->display, c->root, 0, NULL);
    XSetForeground(c->display, root_gc, 0x778899);
    XFillRectangle(c->display, c->root, root_gc, c->x, c->y, 4, 4);
    XSync(c->display, False);
    start = gpu_now_ns();
    gpu_events_wait(c, start - 40000000, 30);
    assert(gpu_now_ns() - start < 150000000); /* Damage bypasses idle deadline. */
    XWarpPointer(c->display, None, c->root, 0, 0, 0, 0, 24, 24);
    XSync(c->display, False);
    start = gpu_now_ns();
    gpu_events_wait(c, start - 40000000, 30);
    assert(gpu_now_ns() - start < 150000000); /* Cursor-only motion. */
    XFillRectangle(c->display, c->root, root_gc, 110, 110, 4, 4);
    XSync(c->display, False);
    start = gpu_now_ns();
    gpu_events_wait(c, start, 30);
    assert(gpu_now_ns() - start >= 190000000); /* Foreign output damage ignored. */
    pid_t child = fork(); assert(child >= 0);
    if (!child) { c->leased = 1; gpu_events_wait(c, 0, 30); _exit(0); }
    int status; assert(waitpid(child, &status, 0) == child);
    assert(WIFEXITED(status) && WEXITSTATUS(status) == 2);
    XFreeGC(c->display, root_gc);
    gpu_events_close(c);
    unsetenv("BLENT_GPU_CADENCE");
}

int main(void) {
    codec_error();
    gpu_capture c = {.x=20, .y=20, .width=32, .height=24};
    c.display = XOpenDisplay(NULL); assert(c.display);
    c.root = DefaultRootWindow(c.display);
    c.pixmap = XCreatePixmap(c.display, c.root, c.width, c.height, 24);
    c.gc = XCreateGC(c.display, c.pixmap, 0, NULL);
    Cursor shape = cursor(c.display, c.root);
    XDefineCursor(c.display, c.root, shape);
    XWarpPointer(c.display, None, c.root, 0, 0, 0, 0, 22, 22); XSync(c.display, False);
    fill(&c); gpu_cursor(&c); verify(&c, 1);
    XWarpPointer(c.display, None, c.root, 0, 0, 0, 0, 100, 100); XSync(c.display, False);
    fill(&c); gpu_cursor(&c); verify(&c, 0);
    cadence(&c);
    XFreeCursor(c.display, shape); XFreeGC(c.display, c.gc);
    XFreePixmap(c.display, c.pixmap); XCloseDisplay(c.display);
    return 0;
}
