package com.blent

import org.json.JSONObject

/** Validate wire numbers before JSONObject can truncate or wrap them into an Int. */
internal object JsonNumbers {
    fun integer(message: JSONObject, name: String, minimum: Int, maximum: Int): Int {
        val value = message.get(name)
        require(value is Number) { "Expected numeric $name" }
        val number = value.toDouble()
        require(number.isFinite() && number >= minimum && number <= maximum && number == number.toInt().toDouble()) {
            "Invalid integer $name"
        }
        return number.toInt()
    }

    fun optional(message: JSONObject, name: String, fallback: Int, minimum: Int, maximum: Int): Int =
        if (message.has(name)) integer(message, name, minimum, maximum) else fallback
}
