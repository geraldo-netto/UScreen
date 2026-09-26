#ifndef BLENT_GPU_H
#define BLENT_GPU_H
#include <stdint.h>
#include "options.h"
#include <X11/Xlib.h>
#include <X11/extensions/Xrandr.h>
#include <X11/extensions/sync.h>
#include <va/va.h>
#include <libavcodec/avcodec.h>
#include <libavutil/hwcontext.h>

/* Linux-only adapter. Native handles never enter the portable session/wire API.
 * One immutable RGB lease is sufficient: VPP completion precedes its release;
 * independent NV12 AVFrames remain owned by libavcodec until final unref. */
typedef struct {
    Display *display;
    Window root;
    Pixmap pixmap;
    GC gc;
    XSyncFence fence;
    RROutput output;
    RRCrtc crtc;
    int x, y, width, height, leased;
    int dma_fd;
    int render_fd;
    uint32_t stride, bytes;
    unsigned char edid[512];
    size_t edid_length;
} gpu_capture;
typedef struct {
    AVBufferRef *device, *frames;
    AVCodecContext *codec;
    AVPacket *packet;
    VADisplay va;
    VASurfaceID rgb;
    VAConfigID config;
    VAContextID context;
} gpu_encoder;

void gpu_av(int result, const char *operation);
void gpu_capture_open(gpu_capture *capture, const gpu_options *options);
void gpu_capture_take(gpu_capture *capture, const gpu_options *options, unsigned sequence);
void gpu_capture_release(gpu_capture *capture);
void gpu_capture_close(gpu_capture *capture);
void gpu_cursor(gpu_capture *capture);
void gpu_identity_load(gpu_capture *capture, const gpu_options *options);
void gpu_render_device(gpu_capture *capture, const gpu_options *options);
int gpu_output_matches(gpu_capture *capture, const gpu_options *options, RROutput output, const char *name);
void gpu_encoder_open(gpu_encoder *encoder, const gpu_capture *capture, const gpu_options *options);
void gpu_encode(gpu_encoder *encoder, gpu_capture *capture, const gpu_options *options, int64_t timestamp);
void gpu_encoder_close(gpu_encoder *encoder);
void gpu_emit_header(const gpu_options *options);
void gpu_emit_packet(const AVPacket *packet);
#endif
