#include "gpu.h"
#include <X11/Xlib-xcb.h>
#include <X11/extensions/Xfixes.h>
#include <xcb/dri3.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static void check_transform(gpu_capture *c) {
    XRRCrtcTransformAttributes *attributes = NULL;
    gpu_require(XRRGetCrtcTransform(c->display, c->crtc, &attributes) && attributes,
                "CRTC transform metadata");
    int32_t matrix[9];
    _Static_assert(sizeof(attributes->currentTransform.matrix) == sizeof(matrix), "32-bit XFixed matrix");
    memcpy(matrix, attributes->currentTransform.matrix, sizeof(matrix));
    int unchanged = gpu_identity_transform(matrix);
    XFree(attributes);
    gpu_require(unchanged, "untransformed capture CRTC");
}

static XRRCrtcInfo *geometry(gpu_capture *c, XRRScreenResources *resources, const gpu_options *options) {
    XRROutputInfo *output = XRRGetOutputInfo(c->display, resources, c->output);
    gpu_require(output && output->connection == RR_Connected && output->crtc, "active capture output");
    gpu_require(gpu_output_matches(c, options, c->output, output->name), "unchanged output identity");
    RRCrtc crtc = output->crtc;
    XRRFreeOutputInfo(output);
    gpu_require(!c->crtc || c->crtc == crtc, "unchanged CRTC ownership");
    c->crtc = crtc;
    check_transform(c);
    XRRCrtcInfo *info = XRRGetCrtcInfo(c->display, resources, crtc);
    gpu_require(info && info->rotation == RR_Rotate_0, "unrotated capture CRTC");
    return info;
}

static void locate_output(gpu_capture *c, const gpu_options *options) {
    XRRScreenResources *resources = XRRGetScreenResourcesCurrent(c->display, c->root);
    gpu_require(resources != NULL, "RandR inventory");
    unsigned matches = 0;
    for (int i = 0; i < resources->noutput; i++) {
        XRROutputInfo *info = XRRGetOutputInfo(c->display, resources, resources->outputs[i]);
        gpu_require(info != NULL, "RandR output");
        if (gpu_output_matches(c, options, resources->outputs[i], info->name)) {
            matches++; c->output = resources->outputs[i];
        }
        XRRFreeOutputInfo(info);
    }
    gpu_require(matches == 1, "unique EVDI connector");
    XRRCrtcInfo *info = geometry(c, resources, options);
    gpu_require(info->width / options->scale / 2 * 2 == options->width &&
                info->height / options->scale / 2 * 2 == options->height, "capture dimensions");
    c->x = info->x; c->y = info->y;
    XRRFreeCrtcInfo(info); XRRFreeScreenResources(resources);
}

static void check_geometry(gpu_capture *c, const gpu_options *options) {
    if (options->pattern) return;
    XRRScreenResources *resources = XRRGetScreenResourcesCurrent(c->display, c->root);
    gpu_require(resources != NULL, "RandR current inventory");
    XRRCrtcInfo *info = geometry(c, resources, options);
    gpu_require(info->x == c->x && info->y == c->y, "unchanged capture position");
    gpu_require(info->width / options->scale / 2 * 2 == options->width &&
                info->height / options->scale / 2 * 2 == options->height, "unchanged capture dimensions");
    XRRFreeCrtcInfo(info); XRRFreeScreenResources(resources);
}

static void export_pixmap(gpu_capture *c) {
    XFlush(c->display);
    xcb_connection_t *connection = XGetXCBConnection(c->display);
    xcb_dri3_buffer_from_pixmap_reply_t *reply = xcb_dri3_buffer_from_pixmap_reply(connection,
        xcb_dri3_buffer_from_pixmap(connection, c->pixmap), NULL);
    gpu_require(reply && reply->nfd == 1, "DRI3 single-object pixmap export");
    c->dma_fd = xcb_dri3_buffer_from_pixmap_reply_fds(connection, reply)[0];
    gpu_require(reply->width == c->width && reply->height == c->height, "export dimensions");
    gpu_require(reply->depth == 24 && reply->bpp == 32, "XRGB8888 export format");
    gpu_require(reply->stride >= c->width * 4 && reply->size >= (uint64_t)reply->stride * c->height,
                "export memory bounds");
    c->stride = reply->stride; c->bytes = reply->size;
    free(reply);
}

void gpu_capture_open(gpu_capture *c, const gpu_options *options) {
    c->dma_fd = -1;
    gpu_identity_load(c, options);
    c->width = options->width * options->scale; c->height = options->height * options->scale;
    c->display = XOpenDisplay(NULL); gpu_require(c->display != NULL, "X11 connection");
    c->root = DefaultRootWindow(c->display);
    gpu_render_device(c, options);
    gpu_require(DefaultDepth(c->display, DefaultScreen(c->display)) == 24, "24-bit X11 screen");
    int major, minor;
    gpu_require(XSyncInitialize(c->display, &major, &minor) && (major * 100 + minor >= 301), "SYNC 3.1 fences");
    if (!options->pattern) locate_output(c, options);
    c->pixmap = XCreatePixmap(c->display, c->root, c->width, c->height, 24);
    XGCValues values = {.subwindow_mode=IncludeInferiors, .graphics_exposures=False};
    c->gc = XCreateGC(c->display, c->pixmap, GCSubwindowMode | GCGraphicsExposures, &values);
    c->fence = XSyncCreateFence(c->display, c->pixmap, False);
    export_pixmap(c);
}

static void pattern(gpu_capture *c, unsigned sequence) {
    XSetForeground(c->display, c->gc, 0x55aa33);
    XFillRectangle(c->display, c->pixmap, c->gc, 0, 0, c->width, c->height);
    XSetForeground(c->display, c->gc, 0xdd3355);
    XFillRectangle(c->display, c->pixmap, c->gc, sequence * 7 % (c->width - 1), 0,
                    c->width / 4 + 1, c->height / 3 + 1);
}

void gpu_capture_take(gpu_capture *c, const gpu_options *options, unsigned sequence) {
    gpu_require(!c->leased, "final RGB consumer release");
    check_geometry(c, options);
    c->leased = 1;
    if (options->pattern) pattern(c, sequence);
    else {
        XCopyArea(c->display, c->root, c->pixmap, c->gc, c->x, c->y, c->width, c->height, 0, 0);
        gpu_cursor(c);
    }
    /* Trigger is asynchronous. Await + reply proves producer GPU completion;
     * an XFlush or round trip alone would only prove request dispatch. */
    XSyncTriggerFence(c->display, c->fence);
    XSyncAwaitFence(c->display, &c->fence, 1);
    XSync(c->display, False);
    /* Reject a layout transition which occurred while the copy was queued. */
    check_geometry(c, options);
}

void gpu_capture_release(gpu_capture *c) {
    gpu_require(c->leased, "owned RGB lease");
    XSyncResetFence(c->display, c->fence);
    c->leased = 0;
}

void gpu_capture_close(gpu_capture *c) {
    gpu_require(!c->leased, "capture retirement after GPU completion");
    close(c->dma_fd);
    close(c->render_fd);
    XSyncDestroyFence(c->display, c->fence);
    XFreeGC(c->display, c->gc); XFreePixmap(c->display, c->pixmap);
    XCloseDisplay(c->display);
}
