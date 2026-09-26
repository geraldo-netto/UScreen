#include "gpu.h"
#include <va/va_drmcommon.h>
#include <va/va_vpp.h>
#include <libdrm/drm_fourcc.h>
#include <libavutil/hwcontext_vaapi.h>
#include <libavutil/opt.h>
#include <stdio.h>
#include <stdlib.h>

void gpu_av(int result, const char *operation) {
    if (result >= 0) return;
    char message[128]; av_strerror(result, message, sizeof(message));
    fprintf(stderr, "[gpu-capture] %s: %s\n", operation, message);
    exit(2);
}

static void import_rgb(gpu_encoder *e, const gpu_capture *c) {
    VADRMPRIMESurfaceDescriptor descriptor = {.fourcc=VA_FOURCC_BGRX,
        .width=c->width, .height=c->height, .num_objects=1, .num_layers=1};
    descriptor.objects[0].fd = c->dma_fd; descriptor.objects[0].size = c->bytes;
    /* DRI3 1.0 exports implicit layout. Never pretend this is linear. */
    descriptor.objects[0].drm_format_modifier = DRM_FORMAT_MOD_INVALID;
    descriptor.layers[0].drm_format = DRM_FORMAT_XRGB8888;
    descriptor.layers[0].num_planes = 1; descriptor.layers[0].pitch[0] = c->stride;
    VASurfaceAttrib attributes[2] = {0};
    attributes[0].type = VASurfaceAttribMemoryType;
    attributes[0].flags = VA_SURFACE_ATTRIB_SETTABLE;
    attributes[0].value.type = VAGenericValueTypeInteger;
    attributes[0].value.value.i = VA_SURFACE_ATTRIB_MEM_TYPE_DRM_PRIME_2;
    attributes[1].type = VASurfaceAttribExternalBufferDescriptor;
    attributes[1].flags = VA_SURFACE_ATTRIB_SETTABLE;
    attributes[1].value.type = VAGenericValueTypePointer;
    attributes[1].value.value.p = &descriptor;
    gpu_require(vaCreateSurfaces(e->va, VA_RT_FORMAT_RGB32, c->width, c->height,
        &e->rgb, 1, attributes, 2) == VA_STATUS_SUCCESS, "external RGB import");
}

static void frame_pool(gpu_encoder *e, const gpu_options *options) {
    e->frames = av_hwframe_ctx_alloc(e->device);
    gpu_require(e->frames != NULL, "NV12 frame pool");
    AVHWFramesContext *frames = (AVHWFramesContext *)e->frames->data;
    frames->format = AV_PIX_FMT_VAAPI; frames->sw_format = AV_PIX_FMT_NV12;
    frames->width = options->width; frames->height = options->height;
    frames->initial_pool_size = 4;
    gpu_av(av_hwframe_ctx_init(e->frames), "NV12 pool initialization");
}

static void codec_open(gpu_encoder *e, const gpu_options *options) {
    const AVCodec *codec = avcodec_find_encoder_by_name("h264_vaapi");
    gpu_require(codec != NULL, "stock h264_vaapi encoder");
    e->codec = avcodec_alloc_context3(codec);
    gpu_require(e->codec != NULL, "codec allocation");
    e->codec->width = options->width; e->codec->height = options->height;
    e->codec->pix_fmt = AV_PIX_FMT_VAAPI;
    e->codec->time_base = (AVRational){1, 1000000};
    e->codec->framerate = (AVRational){options->fps, 1};
    e->codec->max_b_frames = 0; e->codec->gop_size = options->fps;
    e->codec->rc_max_rate = (int64_t)options->bitrate * 1000;
    e->codec->profile = FF_PROFILE_H264_CONSTRAINED_BASELINE;
    e->codec->hw_frames_ctx = av_buffer_ref(e->frames);
    gpu_require(e->codec->hw_frames_ctx != NULL, "codec frame pool reference");
    e->codec->color_range = AVCOL_RANGE_MPEG; e->codec->colorspace = AVCOL_SPC_BT709;
    e->codec->color_primaries = AVCOL_PRI_BT709; e->codec->color_trc = AVCOL_TRC_BT709;
    gpu_av(av_opt_set_int(e->codec->priv_data, "qp", options->quality, 0), "constant QP");
    gpu_av(av_opt_set(e->codec->priv_data, "rc_mode", "CQP", 0), "constant QP mode");
    gpu_av(av_opt_set_int(e->codec->priv_data, "async_depth", 1, 0), "bounded encode depth");
    gpu_av(avcodec_open2(e->codec, codec, NULL), "codec initialization");
}

void gpu_encoder_open(gpu_encoder *e, const gpu_capture *c, const gpu_options *options) {
    char node[64];
    snprintf(node, sizeof(node), "/proc/self/fd/%d", c->render_fd);
    /* Open the validated descriptor, not a path which could resolve to a
     * different device between identity admission and encoder creation. */
    gpu_av(av_hwdevice_ctx_create(&e->device, AV_HWDEVICE_TYPE_VAAPI, node, NULL, 0), "VAAPI device");
    AVHWDeviceContext *device = (AVHWDeviceContext *)e->device->data;
    e->va = ((AVVAAPIDeviceContext *)device->hwctx)->display;
    import_rgb(e, c); frame_pool(e, options); codec_open(e, options);
    gpu_require(vaCreateConfig(e->va, VAProfileNone, VAEntrypointVideoProc, NULL, 0, &e->config) == 0,
        "video processing configuration");
    gpu_require(vaCreateContext(e->va, e->config, options->width, options->height,
        VA_PROGRESSIVE, NULL, 0, &e->context) == 0, "video processing context");
    e->packet = av_packet_alloc(); gpu_require(e->packet != NULL, "packet allocation");
}

static void convert(gpu_encoder *e, AVFrame *frame) {
    VAProcPipelineParameterBuffer parameters = {.surface=e->rgb,
        .surface_color_standard=VAProcColorStandardBT709, .output_color_standard=VAProcColorStandardBT709};
    /* Match the CPU path's numerical RGB-to-limited-BT709 contract. SRGB as
     * source standard applies an additional transfer change on local Mesa. */
    parameters.input_color_properties.color_range = VA_SOURCE_RANGE_FULL;
    parameters.output_color_properties.color_range = VA_SOURCE_RANGE_REDUCED;
    VABufferID buffer;
    gpu_require(vaCreateBuffer(e->va, e->context, VAProcPipelineParameterBufferType,
        sizeof(parameters), 1, &parameters, &buffer) == 0, "conversion parameters");
    VASurfaceID nv12 = (VASurfaceID)(uintptr_t)frame->data[3];
    gpu_require(vaBeginPicture(e->va, e->context, nv12) == 0, "begin conversion");
    gpu_require(vaRenderPicture(e->va, e->context, &buffer, 1) == 0, "submit conversion");
    gpu_require(vaEndPicture(e->va, e->context) == 0, "end conversion");
    gpu_require(vaSyncSurface(e->va, nv12) == 0, "final RGB consumer completion");
    vaDestroyBuffer(e->va, buffer);
}

static void drain(gpu_encoder *e) {
    int result;
    while ((result = avcodec_receive_packet(e->codec, e->packet)) == 0) {
        gpu_emit_packet(e->packet); av_packet_unref(e->packet);
    }
    if (result != AVERROR(EAGAIN) && result != AVERROR_EOF) gpu_av(result, "receive encoded packet");
}

void gpu_encode(gpu_encoder *e, gpu_capture *capture, const gpu_options *options, int64_t timestamp) {
    (void)options;
    AVFrame *frame = av_frame_alloc(); gpu_require(frame != NULL, "frame allocation");
    gpu_av(av_hwframe_get_buffer(e->frames, frame, 0), "acquire NV12 surface");
    frame->pts = timestamp;
    if (capture->damage && gpu_refresh_due(e->last_refresh_us, timestamp)) {
        frame->pict_type = AV_PICTURE_TYPE_I;
        e->last_refresh_us = timestamp;
    }
    convert(e, frame);
    gpu_capture_release(capture);
    gpu_av(avcodec_send_frame(e->codec, frame), "submit owned NV12 frame");
    av_frame_free(&frame);
    drain(e);
}

void gpu_encoder_close(gpu_encoder *e) {
    gpu_av(avcodec_send_frame(e->codec, NULL), "flush encoder"); drain(e);
    av_packet_free(&e->packet); avcodec_free_context(&e->codec);
    vaDestroyContext(e->va, e->context); vaDestroyConfig(e->va, e->config);
    vaDestroySurfaces(e->va, &e->rgb, 1);
    av_buffer_unref(&e->frames); av_buffer_unref(&e->device);
}
