package com.blent.benchmark

import java.io.DataOutputStream
import java.io.File
import org.junit.Assert.*
import org.junit.Test

/** T419 research-fixture admission: malformed metadata cannot size native writes. */
class RectClipTest {
    private fun fixture(edit: (DataOutputStream) -> Unit): File {
        val file = File.createTempFile("t419", ".rect")
        DataOutputStream(file.outputStream()).use(edit)
        return file
    }
    private fun header(out: DataOutputStream, count: Int = 1) {
        listOf(0x54523431, 1, 1280, 800, 60, count).forEach(out::writeInt)
    }
    private fun frame(out: DataOutputStream, x: Int = 0, y: Int = 0, width: Int = 1280,
                      height: Int = 800, size: Int = 3) {
        listOf(x, y, width, height, size).forEach(out::writeInt)
        out.writeLong(42)
        if (size in 1..3) out.write(byteArrayOf(11, 22, 33), 0, size)
    }
    private fun rejected(write: (DataOutputStream) -> Unit) {
        val file = fixture(write)
        try {
            assertThrows(Exception::class.java) { RectClip(file, false).use { fail("Admitted malformed fixture") } }
        } finally { file.delete() }
    }
    @Test fun t419MappedAndBoundedInputReturnIdenticalBytes() {
        val file = fixture { header(it); frame(it) }
        try {
            for (mapped in listOf(false, true)) RectClip(file, mapped).use { clip ->
                val frame = clip.frames.single()
                val start = clip.dataOffset(frame)
                assertEquals(listOf<Byte>(11, 22, 33), (0..2).map { clip.bytes.get(start + it) })
                if (!mapped) assertEquals(3, clip.bytes.capacity())
            }
        } finally { file.delete() }
    }
    @Test fun t419RequiresIndependentFirstPicture() {
        rejected { header(it); frame(it, width = 20) }
        rejected { header(it); frame(it, width = 0, height = 0, size = 0) }
    }
    @Test fun t419RejectsOutOfBoundsAndAmbiguousEmptyRectangles() {
        rejected { header(it); frame(it, x = Int.MAX_VALUE) }
        rejected { header(it); frame(it, width = -1) }
        rejected { header(it, 2); frame(it); frame(it, width = 0, height = 1, size = 0) }
    }
    @Test fun t419RejectsOversizedTruncatedAndTrailingData() {
        rejected { header(it); frame(it, size = 3_100_001) }
        rejected { header(it); frame(it, size = 4) }
        rejected { header(it); frame(it); it.writeByte(1) }
        rejected { header(it, 601) }
    }
    @Test fun t419EmptyUpdateKeepsAnExistingBaseWithoutInputAllocation() {
        val file = fixture { header(it, 2); frame(it); frame(it, width = 0, height = 0, size = 0) }
        try {
            RectClip(file, false).use { clip ->
                assertEquals(0, clip.frames[1].rawBytes)
                assertEquals(3, clip.bytes.capacity())
            }
        } finally { file.delete() }
    }
}
