package com.blent

import android.Manifest
import android.content.pm.PackageManager
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class CameraManifestTest {
    @Test fun t539_cameraIsOptionalAndSharingRequiresPermission() {
        val app = RuntimeEnvironment.getApplication()
        val info = app.packageManager.getPackageInfo(app.packageName,
            PackageManager.GET_PERMISSIONS or PackageManager.GET_CONFIGURATIONS)
        assertTrue("T539 camera capture permission missing",
            info.requestedPermissions.contains(Manifest.permission.CAMERA))
        val feature = info.reqFeatures.firstOrNull { it.name == PackageManager.FEATURE_CAMERA_ANY }
        assertNotNull("T539 explicitly optional camera feature missing", feature)
        assertEquals(0, feature!!.flags and android.content.pm.FeatureInfo.FLAG_REQUIRED)
        for (name in listOf(PackageManager.FEATURE_CAMERA, PackageManager.FEATURE_CAMERA_AUTOFOCUS)) {
            val optional = info.reqFeatures.firstOrNull { it.name == name }
            assertNotNull("T539 permission must not imply required $name hardware", optional)
            assertEquals(0, optional!!.flags and android.content.pm.FeatureInfo.FLAG_REQUIRED)
        }
    }
}
