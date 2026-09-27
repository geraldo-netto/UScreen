// Copyright (c) 2026 Geraldo Netto
package com.blent

import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.Description
import org.junit.runner.JUnitCore
import org.junit.runner.Request
import org.junit.runner.manipulation.Filter

/** T701: model writes between UI classes must not poison the cached dispatcher. */
class ComposeOrderTest {
    @Test fun t701_modelWritesCannotPoisonLaterSettingsCompositionApi27() = ordered("[27]")
    @Test fun t701_modelWritesCannotPoisonLaterSettingsCompositionApi34() = ordered("")

    private fun ordered(sdkSuffix: String) {
        val runner = JUnitCore()
        for ((type, method) in listOf(
            DecoderDiagnosticsUiTest::class.java to "t417_settingsShowInactiveAndChangingReadOnlyDecoderState",
            ReleaseCheckTest::class.java to "t464_disablingChecksRejectsAlreadyQueuedCompletion",
            SettingsAccessibilityTest::class.java to "t611_advancedControlsDiscloseWithoutApplyingDraftOrStartingCamera",
        )) {
            val request = Request.aClass(type).filterWith(object : Filter() {
                override fun describe() = "T701 $method$sdkSuffix"
                override fun shouldRun(description: Description): Boolean =
                    description.methodName == null || description.methodName == method + sdkSuffix
            })
            val result = runner.run(request)
            assertTrue("T701 ${type.simpleName}: ${result.failures}", result.wasSuccessful())
            org.junit.Assert.assertEquals("T701 must execute the ordered regression", 1, result.runCount)
        }
    }
}
