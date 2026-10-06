package dev.bandbridge.relay

/**
 * Learn from the band (frames 06 and 07): one round of listening for the
 * band button the person wants a rule for.
 *
 * [start] arms the capture for a band app press (D9), so the press fills in
 * the signal and runs no key map. A round ends in exactly one of four ways,
 * and each lets go of the capture: the press arrives, [LISTEN_SECONDS] go
 * by, or the page stops it, for Cancel or because it stopped being the
 * visible top (D16). A capture left armed would swallow every later press,
 * with nothing on screen to say why, so no way out skips the disarm, and
 * even an editor that is never told to stop lets go when its time is up.
 *
 * The press and the time running out end the round by themselves, and say
 * so through [ended]. The receivers run on the main thread, so the press
 * reaches the page there. The page calls [tick] once a second, which keeps
 * the countdown's redraws to one a second and lets the rule be tested
 * without a clock.
 */
class Learn(private val ended: (End) -> Unit) {
    /** How a round ended by itself. A round the page stops ends without a word. */
    sealed interface End {
        /** A band app press arrived. Its signal fills in the field. */
        data class Heard(val signal: String) : End

        /** [LISTEN_SECONDS] went by with no press (frame 07). */
        data object NoPress : End
    }

    /** True from [start] until the round ends. */
    var listening: Boolean = false
        private set

    /** The seconds this round has left, for the countdown. */
    var secondsLeft: Int = LISTEN_SECONDS
        private set

    /**
     * Starts a round of [LISTEN_SECONDS], with the capture armed for a band
     * app press. Try again starts another. The capture holds one taker at a
     * time and [stop] drops it, so only this round's press can end it.
     */
    fun start() {
        listening = true
        secondsLeft = LISTEN_SECONDS
        Capture.arm(Capture.Wants.BandSignal) { payload, _ ->
            val signal = Signals.signalOf(payload) ?: return@arm
            stop()
            ended(End.Heard(signal))
        }
    }

    /** One second gone. The last one ends the round with [End.NoPress]. */
    fun tick() {
        if (!listening) {
            return
        }
        secondsLeft -= 1
        if (secondsLeft <= 0) {
            stop()
            ended(End.NoPress)
        }
    }

    /**
     * Ends the round without a word: Cancel, or the page is no longer the
     * visible top. It lets go of the capture only while a round is under
     * way, so a capture the check has armed since is left alone.
     */
    fun stop() {
        if (!listening) {
            return
        }
        listening = false
        Capture.disarm()
    }
}
