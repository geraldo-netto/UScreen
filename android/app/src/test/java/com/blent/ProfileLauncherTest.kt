package com.blent

import android.content.Intent
import android.content.pm.PackageManager
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment

@RunWith(RobolectricTestRunner::class)
class ProfileLauncherTest {
    @Test fun t624_onlyProductionHasLauncher() {
        val app = RuntimeEnvironment.getApplication()
        val launchers = app.packageManager.queryIntentActivities(
            Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
                .setPackage(app.packageName), 0)
        val production = app.packageName == "io.github.geraldo_netto.blent"
        assertEquals("T624 test APK must not duplicate production launcher", if (production) 1 else 0, launchers.size)
        assertNotNull(app.packageManager.getActivityInfo(
            android.content.ComponentName(app.packageName, MainActivity::class.java.name), 0))
    }
}
