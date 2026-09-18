package com.uscreen.benchmark

import org.json.JSONObject

internal interface ReplayInput : AutoCloseable {
    fun feed(bytes: ByteArray, configuration: Boolean, sequence: Long)
    fun summary(): JSONObject
}
