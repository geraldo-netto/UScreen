package com.blent

/** A route change ends this session; a new explicit Start negotiates the new device. */
internal class AudioRoute {
    private var selected: Int? = null
    fun observe(device: Int?) {
        if (selected == null) { selected = device; return }
        check(device == selected) { "Audio route changed. Start again on your computer." }
    }
}
