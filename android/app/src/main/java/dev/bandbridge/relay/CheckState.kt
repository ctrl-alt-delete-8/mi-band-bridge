package dev.bandbridge.relay

/**
 * Where the connection check is (frames 11 to 15), kept apart from any view
 * so the flow is tested without a device.
 *
 * The check watches for a band app press, the one kind of event the relay
 * hands it besides its own test (A1). One that arrives proves the whole way
 * from the band to this app. When none arrives, a test event shows which
 * side the fault is on. It starts inside this app and skips Gadgetbridge
 * (D10), so an app that hears it is listening, and the fault is in
 * Gadgetbridge. An app that does not hear it is not receiving events (the
 * note, section 4).
 */
sealed interface CheckState {
    /** Watching for a band app press for [LISTEN_SECONDS] (frame 11). */
    data object Watching : CheckState

    /** A band app press arrived: its [label], such as bb01, and when it arrived, [at] (frame 12). */
    data class Arrived(val label: String, val at: Long) : CheckState

    /** The watch ended with no band app press (frame 13). Only now is the test event offered. */
    data object NoPress : CheckState

    /** The test event is out, and the check waits [TEST_EVENT_WAIT_MS] for it (D10). */
    data object TestSent : CheckState

    /** The test event arrived, so this app is listening, and band presses stop in Gadgetbridge (frame 14). */
    data object FaultInGadgetbridge : CheckState

    /** The test event did not arrive, so this app is not receiving events (frame 15). */
    data object FaultInThisApp : CheckState
}

/** What can happen to the check: an event heard, a timer run out, or one of its buttons. */
sealed interface Happening {
    /** The capture took an event: its [payload], when it arrived, [at], and whether it is the check's own [test] event. */
    data class Heard(val payload: String, val at: Long, val test: Boolean) : Happening

    /** The state's timer ran out: the watch's [LISTEN_SECONDS], or the test event's [TEST_EVENT_WAIT_MS]. */
    data object TimedOut : Happening

    /** "Send a test event", or "Send the test again". */
    data object SendTest : Happening

    /** "Try the band again". */
    data object TryBandAgain : Happening
}

/**
 * The check's one decision: the state after [happened].
 *
 * Each happening moves the check from the few states that offer it, and
 * any other pair leaves the state as it is. So a timer that fires late, a
 * second event after the first, or a button of a state no longer shown
 * changes nothing, and the page need not guard against them.
 *
 * A test event heard during the watch was sent by an earlier round, and
 * must not pass for a band press, so the watch goes on. While the test is
 * out, a real press still proves Gadgetbridge works (D10), and "Try the
 * band again" gives the test up for a new watch, since its button looks
 * active. A test that arrives after that lands in the watch.
 */
fun next(state: CheckState, happened: Happening): CheckState = when (happened) {
    is Happening.Heard -> when {
        !state.listening -> state
        !happened.test -> CheckState.Arrived(Signals.labelOf(happened.payload), happened.at)
        state == CheckState.TestSent -> CheckState.FaultInGadgetbridge
        else -> state
    }
    Happening.TimedOut -> when (state) {
        CheckState.Watching -> CheckState.NoPress
        CheckState.TestSent -> CheckState.FaultInThisApp
        else -> state
    }
    Happening.SendTest -> when (state) {
        CheckState.NoPress, CheckState.FaultInThisApp -> CheckState.TestSent
        else -> state
    }
    Happening.TryBandAgain -> when (state) {
        CheckState.NoPress, CheckState.TestSent, CheckState.FaultInGadgetbridge -> CheckState.Watching
        else -> state
    }
}

/**
 * Whether the check listens in this state: it holds the capture, which
 * takes every event (D9), and runs the state's timer. It listens while it
 * watches and while its test is out. Every result lets go, so a press after
 * it is an ordinary press again, with its key maps and its event row.
 */
val CheckState.listening: Boolean
    get() = this == CheckState.Watching || this == CheckState.TestSent
