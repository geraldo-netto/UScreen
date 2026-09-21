#include "gpu.h"
#include <X11/Xatom.h>
#include <X11/Xlib-xcb.h>
#include <xcb/dri3.h>
#include <fcntl.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#include <stdlib.h>

void gpu_render_device(gpu_capture *c, const gpu_options *options) {
    c->render_fd = open(options->device, O_RDWR | O_CLOEXEC);
    gpu_require(c->render_fd >= 0, "configured render node");
    xcb_connection_t *connection = XGetXCBConnection(c->display);
    xcb_dri3_open_reply_t *reply = xcb_dri3_open_reply(connection,
        xcb_dri3_open(connection, c->root, 0), NULL);
    gpu_require(reply && reply->nfd == 1, "X11 rendering device identity");
    int producer = xcb_dri3_open_reply_fds(connection, reply)[0];
    struct stat source, destination;
    gpu_require(fstat(producer, &source) == 0 && fstat(c->render_fd, &destination) == 0, "render device metadata");
    close(producer); free(reply);
    gpu_require(S_ISCHR(source.st_mode) && S_ISCHR(destination.st_mode), "DRM character devices");
    gpu_require(gpu_same_render_node(source.st_rdev, destination.st_rdev),
                "same render node required for implicit DRI3 layout");
}

void gpu_identity_load(gpu_capture *c, const gpu_options *options) {
    /* Production passes the owned card's sysfs EDID. An explicit output name
     * is supported only for deliberate standalone prototype/benchmark use. */
    if (options->connector[0] != '/') return;
    int fd = open(options->connector, O_RDONLY | O_NONBLOCK | O_NOFOLLOW | O_CLOEXEC);
    gpu_require(fd >= 0, "owned connector EDID");
    struct stat info;
    gpu_require(fstat(fd, &info) == 0 && S_ISREG(info.st_mode), "regular EDID source");
    unsigned char bytes[513];
    ssize_t count = read(fd, bytes, sizeof(bytes)); close(fd);
    gpu_require(count > 0 && gpu_edid_valid(bytes, count), "valid bounded EDID");
    c->edid_length = count; memcpy(c->edid, bytes, count);
}

int gpu_output_matches(gpu_capture *c, const gpu_options *options, RROutput output, const char *name) {
    if (!c->edid_length) return !strcmp(options->connector, name);
    Atom edid = XInternAtom(c->display, "EDID", True), type;
    if (edid == None) return 0;
    unsigned long count, remaining;
    int format;
    unsigned char *bytes = NULL;
    int result = XRRGetOutputProperty(c->display, output, edid, 0, 128, False, False,
                                      AnyPropertyType, &type, &format, &count, &remaining, &bytes);
    int valid = result == Success && type == XA_INTEGER && format == 8 && remaining == 0;
    int matches = valid && count == c->edid_length;
    if (matches) matches = memcmp(bytes, c->edid, count) == 0;
    XFree(bytes);
    return matches;
}
