package com.blent


/**
 * "A newer release exists", and nothing more. Sideloaded apps cannot update
 * themselves silently — Android always asks the user — so this just answers
 * the question and hands over the release page.
 */
object UpdateCheck {
    const val RELEASES_PAGE = "https://github.com/geraldo-netto/UScreen/releases/latest"
    private const val API = "https://api.github.com/repos/geraldo-netto/UScreen/releases/latest"

    internal val requests: ReleaseChecks = HttpReleaseChecks(API)

    private data class Version(val core: List<Long>, val pre: List<String>)
    private val syntax = Regex("""(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?""")

    private fun parse(value: String): Version? {
        val match = syntax.matchEntire(value.trim().removePrefix("v")) ?: return null
        val core = (1..3).map { index ->
            match.groupValues[index].toLongOrNull()?.takeIf { it <= 0xffff_ffffL } ?: return null
        }
        val pre = match.groupValues[4].takeIf { it.isNotEmpty() }?.split('.') ?: emptyList()
        if (pre.any { it.all(Char::isDigit) && it.length > 1 && it.startsWith('0') }) return null
        return Version(core, pre)
    }

    fun isNewer(candidate: String, current: String): Boolean {
        val a = parse(candidate) ?: return false
        val b = parse(current) ?: return false
        for (i in 0..2) if (a.core[i] != b.core[i]) return a.core[i] > b.core[i]
        return isNewerPrerelease(a.pre, b.pre)
    }

    private fun isNewerPrerelease(a: List<String>, b: List<String>): Boolean {
        if (a.isEmpty() || b.isEmpty()) return a.isEmpty() && b.isNotEmpty()
        for (i in 0 until minOf(a.size, b.size)) {
            val x = a[i]; val y = b[i]
            if (x == y) continue
            val xn = x.all(Char::isDigit); val yn = y.all(Char::isDigit)
            if (xn != yn) return !xn
            if (xn && x.length != y.length) return x.length > y.length
            return x > y
        }
        return a.size > b.size
    }

}
