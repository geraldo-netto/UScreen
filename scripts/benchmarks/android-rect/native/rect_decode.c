/* T419 isolated replay only: bounded decode into a caller-owned direct buffer. */
#include <jni.h>
#include <stdint.h>
#include "lz4.h"
#include "zstd.h"
#include <EGL/egl.h>
#include <EGL/eglext.h>
uint64_t rect_rgb_hash(const unsigned char *data, size_t pixels, size_t stride);

jlong Java_com_blent_benchmark_RectNative_create(JNIEnv *env, jobject self) {
    (void)env; (void)self;
    return (jlong)(intptr_t)ZSTD_createDCtx();
}

void Java_com_blent_benchmark_RectNative_destroy(JNIEnv *env, jobject self, jlong context) {
    (void)env; (void)self;
    ZSTD_freeDCtx((ZSTD_DCtx *)(intptr_t)context);
}

static int valid_span(jlong capacity, jint offset, jint bytes) {
    return offset >= 0 && bytes > 0 && offset <= capacity && bytes <= capacity - offset;
}

static int valid_decode(jlong context, jint expected) {
    return context && expected > 0 && expected <= 3072000;
}

jboolean Java_com_blent_benchmark_RectNative_decode(
        JNIEnv *env, jobject self, jlong context, jint codec, jobject input,
        jint offset, jint bytes, jobject output, jint expected) {
    (void)self;
    if (!valid_decode(context, expected)) return JNI_FALSE;
    jlong capacity = (*env)->GetDirectBufferCapacity(env, input);
    if (!valid_span(capacity, offset, bytes)) return JNI_FALSE;
    if ((*env)->GetDirectBufferCapacity(env, output) < expected) return JNI_FALSE;
    const char *source = (*env)->GetDirectBufferAddress(env, input);
    char *target = (*env)->GetDirectBufferAddress(env, output);
    if (!source || !target) return JNI_FALSE;
    if (codec == 1) return LZ4_decompress_safe(source + offset, target, bytes, expected) == expected;
    if (codec != 2) return JNI_FALSE;
    size_t decoded = ZSTD_decompressDCtx((ZSTD_DCtx *)(intptr_t)context,
                                        target, (size_t)expected, source + offset, (size_t)bytes);
    return decoded == (size_t)expected;
}

/* Full RGB verification of an RGBA readback; intentionally outside timed trials. */
jlong Java_com_blent_benchmark_RectNative_rgbHash(
        JNIEnv *env, jobject self, jobject buffer, jint pixels) {
    (void)self;
    if (pixels <= 0 || pixels > 1024000) return 0;
    if ((*env)->GetDirectBufferCapacity(env, buffer) < (jlong)pixels * 4) return 0;
    const unsigned char *data = (*env)->GetDirectBufferAddress(env, buffer);
    if (!data) return 0;
    return (jlong)rect_rgb_hash(data, (size_t)pixels, 4);
}

jboolean Java_com_blent_benchmark_RectNative_enableTimestamps(
        JNIEnv *env, jobject self, jlong display, jlong surface) {
    (void)env; (void)self;
    PFNEGLGETFRAMETIMESTAMPSUPPORTEDANDROIDPROC supported =
        (PFNEGLGETFRAMETIMESTAMPSUPPORTEDANDROIDPROC)eglGetProcAddress("eglGetFrameTimestampSupportedANDROID");
    if (!supported) return JNI_FALSE;
    EGLDisplay d = (EGLDisplay)(intptr_t)display;
    EGLSurface s = (EGLSurface)(intptr_t)surface;
    if (!supported(d, s, EGL_DISPLAY_PRESENT_TIME_ANDROID)) return JNI_FALSE;
    return eglSurfaceAttrib(d, s, EGL_TIMESTAMPS_ANDROID, EGL_TRUE);
}

jlong Java_com_blent_benchmark_RectNative_nextFrame(
        JNIEnv *env, jobject self, jlong display, jlong surface) {
    (void)env; (void)self;
    PFNEGLGETNEXTFRAMEIDANDROIDPROC next =
        (PFNEGLGETNEXTFRAMEIDANDROIDPROC)eglGetProcAddress("eglGetNextFrameIdANDROID");
    EGLuint64KHR id;
    if (!next || !next((EGLDisplay)(intptr_t)display, (EGLSurface)(intptr_t)surface, &id)) return -3;
    return (jlong)id;
}

jlong Java_com_blent_benchmark_RectNative_presented(
        JNIEnv *env, jobject self, jlong display, jlong surface, jlong id) {
    (void)env; (void)self;
    PFNEGLGETFRAMETIMESTAMPSANDROIDPROC query =
        (PFNEGLGETFRAMETIMESTAMPSANDROIDPROC)eglGetProcAddress("eglGetFrameTimestampsANDROID");
    EGLint field = EGL_DISPLAY_PRESENT_TIME_ANDROID;
    EGLnsecsANDROID value;
    if (!query) return -3;
    if (!query((EGLDisplay)(intptr_t)display, (EGLSurface)(intptr_t)surface,
               (EGLuint64KHR)id, 1, &field, &value)) return -3;
    return (jlong)value;
}
