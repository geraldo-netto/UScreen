package com.uscreen

internal fun interface ReleaseChecks {
    fun newCall(current: String): ReleaseCall
}

internal interface ReleaseCall {
    fun start(result: (String?) -> Unit)
    fun cancel()
}

/** Main-thread owner; HTTP callbacks only enqueue generation-checked results. */
internal class ReleaseCheckOwner(
    private val checks: ReleaseChecks,
    private val enabled: () -> Boolean,
    private val dispatch: (() -> Unit) -> Unit,
    private val publish: (String?) -> Unit,
) {
    private var active = false
    private var checked = false
    private var generation = 0L
    private var version: String? = null
    private var pending: ReleaseCall? = null

    fun start() { active = true; requestIfNeeded() }
    fun stop() { active = false; cancelPending() }
    fun check(current: String) { version = current; requestIfNeeded() }
    fun preferenceChanged() {
        if (!enabled()) {
            cancelPending()
            checked = false
            publish(null)
        } else requestIfNeeded()
    }

    private fun cancelPending() {
        generation++
        pending?.cancel()
        pending = null
    }

    private fun requestIfNeeded() {
        if (!active || !enabled() || checked || pending != null) return
        val current = version ?: return
        val epoch = ++generation
        val call = checks.newCall(current)
        pending = call
        call.start { result -> dispatch { complete(epoch, result) } }
    }

    private fun complete(epoch: Long, result: String?) {
        if (epoch != generation || !active || !enabled()) return
        pending = null
        checked = true
        publish(result)
    }
}
