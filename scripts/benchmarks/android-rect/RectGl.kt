package com.uscreen.benchmark

import android.opengl.EGL14 as E
import android.opengl.GLES30 as G
import android.view.Surface
import java.nio.ByteBuffer

/** A persistent RGB texture: one rectangle upload followed by a complete draw. */
internal class RectGl(surface: Surface) : AutoCloseable {
    private val display = E.eglGetDisplay(E.EGL_DEFAULT_DISPLAY)
    private val config = arrayOfNulls<android.opengl.EGLConfig>(1)
    private var context = E.EGL_NO_CONTEXT
    private var window = E.EGL_NO_SURFACE
    private var program = 0
    private var texture = 0
    private var framebuffer = 0
    private val size = IntArray(2)
    var timestamps = false
        private set
    init {
        try { initialize(surface) } catch (error: Exception) { close(); throw error }
    }
    private fun initialize(surface: Surface) {
        val version = IntArray(2)
        check(E.eglInitialize(display, version, 0, version, 1))
        val options = intArrayOf(E.EGL_RENDERABLE_TYPE, 0x40, E.EGL_SURFACE_TYPE, E.EGL_WINDOW_BIT,
            E.EGL_RED_SIZE, 8, E.EGL_GREEN_SIZE, 8, E.EGL_BLUE_SIZE, 8, E.EGL_ALPHA_SIZE, 8, E.EGL_NONE)
        check(E.eglChooseConfig(display, options, 0, config, 0, 1, version, 0) && version[0] > 0)
        context = E.eglCreateContext(display, config[0], E.EGL_NO_CONTEXT,
            intArrayOf(E.EGL_CONTEXT_CLIENT_VERSION, 3, E.EGL_NONE), 0)
        window = E.eglCreateWindowSurface(display, config[0], surface, intArrayOf(E.EGL_NONE), 0)
        check(context != E.EGL_NO_CONTEXT && window != E.EGL_NO_SURFACE)
        check(E.eglMakeCurrent(display, window, window, context))
        check(E.eglSwapInterval(display, 1))
        timestamps = RectNative.enableTimestamps(display.nativeHandle, window.nativeHandle)
        check(E.eglQuerySurface(display, window, E.EGL_WIDTH, size, 0))
        check(E.eglQuerySurface(display, window, E.EGL_HEIGHT, size, 1))
        setupTexture()
        program = createProgram()
        G.glUseProgram(program)
        G.glUniform1i(G.glGetUniformLocation(program, "picture"), 0)
        G.glDisable(G.GL_DITHER)
        checkGl()
    }
    private fun setupTexture() {
        val ids = IntArray(1)
        G.glGenTextures(1, ids, 0); texture = ids[0]
        G.glBindTexture(G.GL_TEXTURE_2D, texture)
        G.glTexStorage2D(G.GL_TEXTURE_2D, 1, G.GL_RGB8, 1280, 800)
        G.glTexParameteri(G.GL_TEXTURE_2D, G.GL_TEXTURE_MIN_FILTER, G.GL_NEAREST)
        G.glTexParameteri(G.GL_TEXTURE_2D, G.GL_TEXTURE_MAG_FILTER, G.GL_NEAREST)
        G.glTexParameteri(G.GL_TEXTURE_2D, G.GL_TEXTURE_WRAP_S, G.GL_CLAMP_TO_EDGE)
        G.glTexParameteri(G.GL_TEXTURE_2D, G.GL_TEXTURE_WRAP_T, G.GL_CLAMP_TO_EDGE)
        G.glPixelStorei(G.GL_UNPACK_ALIGNMENT, 1)
        G.glGenFramebuffers(1, ids, 0); framebuffer = ids[0]
    }
    fun upload(frame: RectFrame, pixels: ByteBuffer) {
        pixels.position(0)
        G.glTexSubImage2D(G.GL_TEXTURE_2D, 0, frame.x, frame.y, frame.width, frame.height,
            G.GL_RGB, G.GL_UNSIGNED_BYTE, pixels)
    }
    fun draw(): Long {
        G.glViewport(0, 0, size[0], size[1])
        G.glDrawArrays(G.GL_TRIANGLES, 0, 3)
        val id = if (timestamps) RectNative.nextFrame(display.nativeHandle, window.nativeHandle) else -3L
        check(E.eglSwapBuffers(display, window))
        checkGl()
        return id
    }
    fun presented(id: Long): Long = RectNative.presented(display.nativeHandle, window.nativeHandle, id)
    fun verify(expected: Long, output: ByteBuffer) {
        G.glBindFramebuffer(G.GL_FRAMEBUFFER, framebuffer)
        G.glFramebufferTexture2D(G.GL_FRAMEBUFFER, G.GL_COLOR_ATTACHMENT0, G.GL_TEXTURE_2D, texture, 0)
        check(G.glCheckFramebufferStatus(G.GL_FRAMEBUFFER) == G.GL_FRAMEBUFFER_COMPLETE)
        output.position(0)
        G.glReadPixels(0, 0, 1280, 800, G.GL_RGBA, G.GL_UNSIGNED_BYTE, output)
        G.glBindFramebuffer(G.GL_FRAMEBUFFER, 0)
        checkGl()
        check(RectNative.rgbHash(output, 1280 * 800) == expected) { "Reconstructed texture mismatch" }
    }
    fun identity(): String = "${G.glGetString(G.GL_VENDOR)}; ${G.glGetString(G.GL_RENDERER)}; ${G.glGetString(G.GL_VERSION)}"
    override fun close() {
        if (context != E.EGL_NO_CONTEXT) {
            G.glDeleteTextures(1, intArrayOf(texture), 0)
            G.glDeleteFramebuffers(1, intArrayOf(framebuffer), 0)
            G.glDeleteProgram(program)
        }
        E.eglMakeCurrent(display, E.EGL_NO_SURFACE, E.EGL_NO_SURFACE, E.EGL_NO_CONTEXT)
        if (window != E.EGL_NO_SURFACE) E.eglDestroySurface(display, window)
        if (context != E.EGL_NO_CONTEXT) E.eglDestroyContext(display, context)
        E.eglTerminate(display)
        E.eglReleaseThread()
    }
    private fun checkGl() { check(G.glGetError() == G.GL_NO_ERROR) { "GLES error" } }
    private fun shader(kind: Int, source: String): Int {
        val shader = G.glCreateShader(kind)
        G.glShaderSource(shader, source); G.glCompileShader(shader)
        val status = IntArray(1); G.glGetShaderiv(shader, G.GL_COMPILE_STATUS, status, 0)
        check(status[0] != 0) { G.glGetShaderInfoLog(shader) }
        return shader
    }
    private fun createProgram(): Int {
        val vertex = shader(G.GL_VERTEX_SHADER, """#version 300 es
            out vec2 uv;
            void main() {
                vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
                uv = vec2(p.x, 1.0 - p.y); gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
            }
        """.trimIndent())
        val fragment = shader(G.GL_FRAGMENT_SHADER, """#version 300 es
            precision highp float;
            uniform sampler2D picture; in vec2 uv; out vec4 color;
            void main() { color = vec4(texture(picture, uv).rgb, 1.0); }
        """.trimIndent())
        val program = G.glCreateProgram()
        G.glAttachShader(program, vertex); G.glAttachShader(program, fragment); G.glLinkProgram(program)
        G.glDeleteShader(vertex); G.glDeleteShader(fragment)
        val status = IntArray(1); G.glGetProgramiv(program, G.GL_LINK_STATUS, status, 0)
        check(status[0] != 0) { G.glGetProgramInfoLog(program) }
        return program
    }
}
