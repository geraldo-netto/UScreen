package com.blent

import org.junit.Assert.*
import org.junit.Test

class ProfileSoftwareTest {
    @Test fun t480_stableSoftwareChangesWithEitherInputAndAcceptsBoundaryText() {
        val base = ProfileSoftware.fingerprint("firmware", "build")
        assertEquals(base, ProfileSoftware.fingerprint("firmware", "build"))
        assertNotEquals(base, ProfileSoftware.fingerprint("new", "build"))
        assertNotEquals(base, ProfileSoftware.fingerprint("firmware", "new"))
        for (size in listOf(0, 1, 63, 64, 65, 4096)) {
            val value = "\u0000é".repeat(size)
            assertTrue(ProfileSoftware.fingerprint(value, value).matches(Regex("[0-9a-f]{64}")))
        }
    }
}
