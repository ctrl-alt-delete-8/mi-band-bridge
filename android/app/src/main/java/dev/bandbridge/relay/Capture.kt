package dev.bandbridge.relay

/**
 * The capture Learn and the check arm, to take the next band event without
 * running a key map (D9).
 *
 * While it is armed, the receiver hands it every event it wants and does
 * nothing else with that event: no key map, no event row. The band's own
 * traffic never reaches it, because the relay drops that first (C2). It stays
 * armed until its page disarms it, so every way off the page must disarm it.
 * A capture left armed swallows every later press, with nothing on screen to
 * say why.
 *
 * It is process-wide because the receivers and the pages share nothing else.
 * At most one is armed: a new [arm] replaces the one before.
 */
object Capture {
    /** Which events a capture takes. */
    enum class Wants {
        /** Learn: a band app press, and never a test event. */
        BandSignal,

        /**
         * The check: every event the relay hands on, test events included.
         * The relay drops the band's own buttons first, so only a band app
         * press can end the watch (A1).
         */
        AnyEvent,
    }

    private class Armed(val wants: Wants, val taken: (payload: String, test: Boolean) -> Unit)

    private var current: Armed? = null

    val armed: Boolean
        @Synchronized get() = current != null

    @Synchronized
    fun arm(wants: Wants, taken: (payload: String, test: Boolean) -> Unit) {
        current = Armed(wants, taken)
    }

    @Synchronized
    fun disarm() {
        current = null
    }

    /**
     * True when armed and wanted: the `taken` of [arm] has run, and the caller
     * does nothing else with the event.
     *
     * `taken` runs under the lock, so a disarm from another thread cannot
     * fall between the choice and the call. The lock is reentrant, so `taken`
     * may itself disarm or arm again.
     */
    @Synchronized
    fun offer(payload: String, test: Boolean): Boolean {
        val capture = current ?: return false
        val wanted = when (capture.wants) {
            Wants.BandSignal -> !test && Signals.signalOf(payload) != null
            Wants.AnyEvent -> true
        }
        if (wanted) {
            capture.taken(payload, test)
        }
        return wanted
    }
}
