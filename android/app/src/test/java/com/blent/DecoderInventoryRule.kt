package com.blent

import android.media.MediaCodecInfo
import android.media.MediaFormat
import org.junit.rules.ExternalResource
import org.robolectric.shadows.MediaCodecInfoBuilder
import org.robolectric.shadows.ShadowMediaCodecList

/** Native-codec lifecycle fixtures must advertise the codecs they emulate (T468). */
class DecoderInventoryRule : ExternalResource() {
    override fun before() {
        ShadowMediaCodecList.reset()
        add("video/avc", MediaCodecInfo.CodecProfileLevel.AVCProfileHigh, MediaCodecInfo.CodecProfileLevel.AVCLevel52)
        add("video/hevc", MediaCodecInfo.CodecProfileLevel.HEVCProfileMain, MediaCodecInfo.CodecProfileLevel.HEVCMainTierLevel52)
    }
    override fun after() = ShadowMediaCodecList.reset()
    private fun add(mime: String, profile: Int, level: Int) {
        val capability = MediaCodecInfoBuilder.CodecCapabilitiesBuilder.newBuilder()
            .setMediaFormat(MediaFormat.createVideoFormat(mime, 1920, 1080))
            .setIsEncoder(false).setColorFormats(intArrayOf(19))
            .setProfileLevels(arrayOf(MediaCodecInfo.CodecProfileLevel().apply {
                this.profile = profile; this.level = level
            })).build()
        ShadowMediaCodecList.addCodec(MediaCodecInfoBuilder.newBuilder()
            .setName("test.decoder.$mime").setIsEncoder(false)
            .setIsSoftwareOnly(false).setIsHardwareAccelerated(true)
            .setCapabilities(capability).build())
    }
}
