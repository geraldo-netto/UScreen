// Copyright (c) 2026 Geraldo Netto
package com.blent

/** T707: preserve lifecycle assertions while waiting for owned background retirement. */
internal fun awaitCameraCondition(condition: () -> Boolean) {
    val deadline = System.nanoTime() + java.util.concurrent.TimeUnit.SECONDS.toNanos(3)
    while (!condition() && System.nanoTime() < deadline) {
        org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
        Thread.sleep(5)
    }
    org.junit.Assert.assertTrue("Camera lifecycle did not reach its expected state", condition())
}
