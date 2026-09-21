#include "gpu.h"
#include <X11/Xutil.h>
#include <X11/extensions/Xfixes.h>
#include <X11/extensions/Xrender.h>
#include <stdlib.h>

static int intersects(const gpu_capture *c, const XFixesCursorImage *cursor) {
    int x = cursor->x - cursor->xhot - c->x;
    int y = cursor->y - cursor->yhot - c->y;
    return x < c->width && y < c->height && x + cursor->width > 0 && y + cursor->height > 0;
}

static XImage *cursor_image(gpu_capture *c, const XFixesCursorImage *cursor) {
    unsigned count = (unsigned)cursor->width * cursor->height;
    gpu_require(count <= 1024 * 1024, "bounded cursor image");
    char *pixels = calloc(count, 4); gpu_require(pixels != NULL, "cursor allocation");
    XImage *image = XCreateImage(c->display, DefaultVisual(c->display, DefaultScreen(c->display)),
        32, ZPixmap, 0, pixels, cursor->width, cursor->height, 32, 0);
    gpu_require(image != NULL, "cursor XImage");
    for (unsigned y = 0; y < cursor->height; y++)
        for (unsigned x = 0; x < cursor->width; x++)
            XPutPixel(image, x, y, cursor->pixels[y * cursor->width + x]);
    return image;
}

static void composite(gpu_capture *c, XFixesCursorImage *cursor) {
    XImage *image = cursor_image(c, cursor);
    Pixmap pixmap = XCreatePixmap(c->display, c->root, cursor->width, cursor->height, 32);
    GC gc = XCreateGC(c->display, pixmap, 0, NULL);
    XPutImage(c->display, pixmap, gc, image, 0, 0, 0, 0, cursor->width, cursor->height);
    XRenderPictFormat *argb = XRenderFindStandardFormat(c->display, PictStandardARGB32);
    XRenderPictFormat *rgb = XRenderFindVisualFormat(c->display, DefaultVisual(c->display, DefaultScreen(c->display)));
    gpu_require(argb && rgb, "cursor render formats");
    Picture source = XRenderCreatePicture(c->display, pixmap, argb, 0, NULL);
    Picture destination = XRenderCreatePicture(c->display, c->pixmap, rgb, 0, NULL);
    XRenderComposite(c->display, PictOpOver, source, None, destination, 0, 0, 0, 0,
        cursor->x - cursor->xhot - c->x, cursor->y - cursor->yhot - c->y, cursor->width, cursor->height);
    XRenderFreePicture(c->display, source); XRenderFreePicture(c->display, destination);
    XFreeGC(c->display, gc); XFreePixmap(c->display, pixmap); XDestroyImage(image);
}

void gpu_cursor(gpu_capture *c) {
    /* Only cursor pixels traverse CPU memory. Desktop RGB remains GPU-backed. */
    XFixesCursorImage *cursor = XFixesGetCursorImage(c->display);
    gpu_require(cursor != NULL, "XFixes cursor image");
    if (intersects(c, cursor)) composite(c, cursor);
    XFree(cursor);
}
