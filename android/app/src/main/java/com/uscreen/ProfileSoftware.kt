package com.uscreen

/** Stable software identity only; no transient session scope or credential. */
internal object ProfileSoftware {
    fun fingerprint(firmware: String, source: String): String {
        val digest = java.security.MessageDigest.getInstance("SHA-256")
        digest.update(firmware.toByteArray(Charsets.UTF_8))
        digest.update(0.toByte())
        return digest.digest(source.toByteArray(Charsets.UTF_8)).joinToString("") { "%02x".format(it) }
    }
}
