package com.uscreen

import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.xmlpull.v1.XmlPullParser

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class BackupRulesTest {
    private val app get() = RuntimeEnvironment.getApplication()

    @Test fun t257_packagedRulesExcludeTheTokenFromEveryBackupTransport() {
        // Verify the production preference store, not an unrelated XML filename.
        app.getSharedPreferences("uscreen", 0).edit().putString("host_token", "T257").commit()
        assertEquals("T257", Prefs(app).hostToken)
        assertExcludesToken("fullBackupContent", setOf("full-backup-content"))
        assertExcludesToken("dataExtractionRules", setOf("cloud-backup", "device-transfer"))
    }

    private fun manifestResource(attribute: String): Int {
        app.assets.openXmlResourceParser("AndroidManifest.xml").use { parser ->
            while (parser.next() != XmlPullParser.END_DOCUMENT) {
                if (parser.eventType == XmlPullParser.START_TAG && parser.name == "application") {
                    return parser.getAttributeResourceValue(
                        "http://schemas.android.com/apk/res/android", attribute, 0)
                }
            }
        }
        error("T257: packaged application manifest is missing")
    }

    private fun assertExcludesToken(attribute: String, transports: Set<String>) {
        val id = manifestResource(attribute)
        assertTrue("T257: missing packaged $attribute reference", id != 0)
        val covered = mutableSetOf<String>()
        var transport = ""
        app.resources.getXml(id).use { parser ->
            while (parser.next() != XmlPullParser.END_DOCUMENT) {
                if (parser.eventType != XmlPullParser.START_TAG) continue
                if (parser.name in transports) transport = parser.name
                if (excludesToken(parser)) covered.add(transport)
            }
        }
        assertEquals("T257: $attribute omits an exclusion", transports, covered)
    }

    private fun excludesToken(parser: XmlPullParser): Boolean =
        parser.name == "exclude" && parser.getAttributeValue(null, "domain") == "sharedpref" &&
            parser.getAttributeValue(null, "path") == "uscreen.xml"
}
