package com.blent

import java.io.ByteArrayOutputStream
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test

class AudioOutputTest {
    private class Focus : OutputFocus {
        var pause = false; var closes = 0
        override fun paused() = pause
        override fun close() { closes++ }
    }
    private class Track : OutputTrack {
        var plays = 0; var flushes = 0; var closes = 0; var routeId: Int? = null
        var closing: (() -> Unit)? = null
        var result: (Int) -> OutputWrite = { OutputWrite(minOf(it, 120)) }
        val data = ByteArrayOutputStream()
        override fun play() { plays++ }
        override fun flush() { flushes++ }
        override fun route() = routeId
        override fun description() = "fixture track"
        override fun write(bytes: ByteArray, offset: Int, count: Int): OutputWrite {
            val answer = result(count)
            if (answer.bytes in 0..count) data.write(bytes, offset, answer.bytes)
            return answer
        }
        override fun close() { closes++; closing?.invoke() }
    }
    private fun chunk(at: Long = 0, gap: Boolean = false) = PlaybackChunk(
        ShortArray(960) { if (it % 2 == 0) 1234 else -4321 }, at, gap)
    private suspend fun rejects(action: suspend () -> Unit) {
        try { action(); fail("T719 invalid playback accepted") } catch (_: IllegalArgumentException) {}
    }
    @Test fun t719_partialWritesRetainStereoAndFocusFlushesStaleSamples() = runBlocking {
        val focus = Focus(); val track = Track(); var now = 0L
        val output = AudioOutput({ track }, focus) { now }
        output.start(); assertEquals(1, track.plays); assertEquals("fixture track", output.description())
        assertEquals(SpeakerWrite.Written, output.write(chunk()))
        assertArrayEquals(byteArrayOf(0xd2.toByte(), 4, 0x1f, 0xef.toByte()), track.data.toByteArray().copyOf(4))
        assertEquals(1920, track.data.size())
        focus.pause = true; assertTrue(output.paused)
        assertEquals(SpeakerWrite.Paused, output.write(chunk())); assertEquals(1, track.flushes)
        assertEquals(1920, track.data.size())
        focus.pause = false; assertEquals(SpeakerWrite.Written, output.write(chunk(gap = true)))
        assertEquals(2, track.flushes); assertEquals(3, track.plays)
        now = 201
        assertEquals(SpeakerWrite.Reset, output.write(chunk())); assertEquals(3, track.flushes)
        assertEquals(3840, track.data.size())
        output.close(); output.close(); assertEquals(1, track.closes); assertEquals(1, focus.closes)
        try { output.start(); fail("closed track started") } catch (_: IllegalStateException) {}
    }
    @Test fun t719_deadTrackRecreationDropsCurrentBlockAndResetsRoute() = runBlocking {
        val focus = Focus(); val first = Track(); val replacement = Track(); var opens = 0
        first.routeId = 1; replacement.routeId = 2
        first.result = { OutputWrite(0, true) }
        AudioOutput({ if (opens++ == 0) first else replacement }, focus) { 0 }.use { output ->
            output.start(); assertEquals(SpeakerWrite.Reset, output.write(chunk()))
            assertEquals(1, first.closes); assertEquals(1, replacement.plays)
            assertEquals(SpeakerWrite.Written, output.write(chunk()))
            replacement.routeId = 3
            try { output.write(chunk()); fail("changed route allowed") } catch (_: IllegalStateException) {}
        }
        assertEquals(1, replacement.closes); assertEquals(1, focus.closes)
    }
    @Test fun t719_invalidNativeCountsClockAndCompleteWriteDeadline() = runBlocking {
        for (count in listOf(Int.MIN_VALUE, -1, 1, 2, 3, 1919, 1921, Int.MAX_VALUE)) {
            val track = Track(); track.result = { OutputWrite(count) }
            AudioOutput({ track }, Focus()) { 0 }.use { output -> rejects { output.write(chunk()) } }
        }
        val track = Track(); val focus = Focus(); var now = 0L
        AudioOutput({ track }, focus) { now }.use { output ->
            for (size in listOf(0, 1, 959, 961, 1920)) rejects { output.write(chunk().copy(samples = ShortArray(size))) }
            rejects { output.write(chunk(-1)) }
            try { output.write(chunk(1)); fail("future block accepted") } catch (_: IllegalStateException) {}
            track.result = { now += 50; OutputWrite(4) }
            try { output.write(chunk()); fail("partial progress restarted deadline") }
            catch (error: IllegalStateException) { assertEquals("Speaker playback stalled.", error.message) }
            now = 0; var writes = 0
            track.result = { if (writes++ == 0) OutputWrite(0) else OutputWrite(it) }
            assertEquals(SpeakerWrite.Written, output.write(chunk())); assertEquals(2, writes)
            track.result = { now = -1; OutputWrite(4) }
            try { output.write(chunk()); fail("backwards write clock") } catch (_: IllegalStateException) {}
        }
        val badDead = Track(); badDead.result = { OutputWrite(4, true) }
        AudioOutput({ badDead }, Focus()) { 0 }.use { output -> rejects { output.write(chunk()) } }
    }
    @Test fun t719_openAndCloseFailuresStillReleaseFocus() {
        val opening = Focus()
        try { AudioOutput({ error("open failed") }, opening) { 0 }; fail("open failure hidden") }
        catch (error: IllegalStateException) { assertEquals("open failed", error.message) }
        assertEquals(1, opening.closes)
        val closing = Focus(); val track = Track(); track.closing = { error("close failed") }
        val output = AudioOutput({ track }, closing) { 0 }
        try { output.close(); fail("close failure hidden") } catch (_: IllegalStateException) {}
        output.close(); assertEquals(1, closing.closes); assertEquals(1, track.closes)
    }
}
