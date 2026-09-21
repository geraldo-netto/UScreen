/* T575 optional native adapter suite. Run in an isolated Xvfb server: never
 * moves the real desktop pointer, changes a monitor, or needs a GPU. */
#include "../gpu/gpu.h"
#include <X11/Xutil.h>
#include <assert.h>
#include <string.h>
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
    XFreeCursor(c.display, shape); XFreeGC(c.display, c.gc);
    XFreePixmap(c.display, c.pixmap); XCloseDisplay(c.display);
    return 0;
}
