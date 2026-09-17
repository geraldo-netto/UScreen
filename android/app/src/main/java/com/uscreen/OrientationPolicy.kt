package com.uscreen

import android.content.pm.ActivityInfo
import android.view.Surface

/** Sensor angles use the panel's natural axes, independently of app rotation. */
internal object OrientationPolicy {
    fun naturallyLandscape(width: Int, height: Int, rotation: Int): Boolean {
        val quarterTurn = rotation == Surface.ROTATION_90 || rotation == Surface.ROTATION_270
        return (width > height) != quarterTurn
    }

    fun landscapeForSensor(angle: Int, naturallyLandscape: Boolean): Int? {
        if (angle !in 0..359) return null
        val portraitAngle = if (naturallyLandscape) (angle + 270) % 360 else angle
        // Preserve ±35-degree acceptance bands and leave the current pin alone
        // while upright, near a boundary, or when the sensor reports unknown.
        return when (portraitAngle) {
            in 235..305 -> ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE
            in 55..125 -> ActivityInfo.SCREEN_ORIENTATION_REVERSE_LANDSCAPE
            else -> null
        }
    }
}
