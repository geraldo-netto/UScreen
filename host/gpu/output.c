#include "gpu.h"
#include <libavutil/adler32.h>
#include <inttypes.h>
#include <stdio.h>

void gpu_emit_header(const gpu_options *options) {
    printf("#software: Blent GPU capture, stock FFmpeg %s\n", av_version_info());
    printf("#tb 0: 1/1000000\n#media_type 0: video\n#codec_id 0: h264\n");
    printf("#dimensions 0: %ux%u\n", options->width, options->height);
    gpu_require(fflush(stdout) == 0, "encoded stream header");
}

void gpu_emit_packet(const AVPacket *packet) {
    gpu_require(packet->size > 0 && packet->size <= 8 * 1024 * 1024, "bounded encoded packet");
    unsigned checksum = av_adler32_update(0, packet->data, packet->size);
    printf("0, %" PRId64 ", %" PRId64 ", %" PRId64 ", %d, 0x%08x\n",
        packet->dts, packet->pts, packet->duration, packet->size, checksum);
    gpu_require(fwrite(packet->data, 1, packet->size, stdout) == (size_t)packet->size, "encoded payload");
    gpu_require(fflush(stdout) == 0, "encoded packet flush");
}
