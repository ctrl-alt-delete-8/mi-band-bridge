package dev.bandbridge.relay.ui

import android.content.Context
import android.graphics.drawable.GradientDrawable
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.TextView
import androidx.core.view.isVisible
import dev.bandbridge.relay.BandReceiver
import dev.bandbridge.relay.Capture
import dev.bandbridge.relay.CheckState
import dev.bandbridge.relay.Fixtures
import dev.bandbridge.relay.Happening
import dev.bandbridge.relay.LISTEN_SECONDS
import dev.bandbridge.relay.R
import dev.bandbridge.relay.TEST_EVENT_WAIT_MS
import dev.bandbridge.relay.Words
import dev.bandbridge.relay.listening
import dev.bandbridge.relay.next

private const val MATCH = ViewGroup.LayoutParams.MATCH_PARENT
private const val WRAP = ViewGroup.LayoutParams.WRAP_CONTENT

/**
 * The connection check (frames 11 to 15), which Setup's step 7 opens: does
 * a band press reach this app, and if not, on which side is the fault.
 *
 * [next] decides where the check goes. The page arms the capture, times the
 * state, and draws it. While the check watches, and while its test event is
 * out, the capture takes every event (D9), so no key map runs and no event
 * row is added during a check. Every result lets go of it.
 *
 * A capture left armed would swallow every later press, with nothing on
 * screen to say why, so every way off the page lets go too: Cancel, Done,
 * "Back to setup steps", Back, Home, a page over the check and the app going
 * to the background all reach [leave] (D16). Each listening state also has
 * its timer, so a check that is never left, such as one under the recent
 * apps screen, which does not pause the app, lets go within ten seconds. A
 * check that was watching or waiting for its test starts over, from a full
 * watch, when it is the visible top again. A result stays until the person
 * acts.
 *
 * The states replace each other in place, and the new buttons lie where
 * the old ones were: frame 14's "Back to setup steps" where frame 13's
 * "Send a test event" was. So each change starts the navigator's quiet
 * time, and a second tap on an e-ink panel cannot act on a result that the
 * panel has not drawn yet. For the same reason nothing announces a new
 * state the way a new page is announced, so each state's title is live, and
 * a screen reader reads it as [Widgets.reveal] shows the state.
 */
class CheckScreen(private val context: Context, private val navigator: Navigator) : Page {
    override val title = "Check connection"

    /** Where the check is. A new check watches, from its first [enter]. */
    private var state: CheckState = CheckState.Watching

    /** Whether this page armed the capture and has not let go of it since. */
    private var holding = false

    /**
     * The watch's seconds and the test's wait, on the main thread. The
     * receivers call there too, so a timer and an event never overlap.
     */
    private val clock = Handler(Looper.getMainLooper())
    private val nextSecond = Runnable { second() }
    private val testWaitOver = Runnable { happen(Happening.TimedOut) }
    private var watchStartedAt = 0L
    private var secondsLeft = LISTEN_SECONDS

    private val countdown = Widgets.countdown(context)

    /** Frame 12's line, which names the event that arrived and when. */
    private val arrivedLine = text("")

    /** Frames 11 to 15, one panel each, of which [show] shows one. TestSent shows frame 13's. */
    private val watching = panel(
        Widgets.resultMark(context, R.drawable.ic_radio, filled = false),
        title("Press a button in the band app now."),
        text("No key map runs during the check."),
        space(8f),
        countdown.view,
    )
    private val arrived = panel(
        Widgets.resultMark(context, R.drawable.ic_check, filled = false),
        title("It works."),
        arrivedLine,
    )
    private val noPress = panel(
        Widgets.resultMark(context, R.drawable.ic_x, filled = true),
        title("No press arrived in $LISTEN_SECONDS seconds."),
        text("A test event shows whether the fault is in this app or in Gadgetbridge."),
    )
    private val faultInGadgetbridge = panel(
        Widgets.resultMark(context, R.drawable.ic_arrow_left_right, filled = false),
        title("The fault is in Gadgetbridge."),
        text("The test event arrived, so this app is listening. Check these settings:"),
        Widgets.checklist(
            context,
            "The band is connected (step 1).",
            "The broadcast switch is on (step 3).",
            "Both values match exactly (step 4).",
            "You restarted Gadgetbridge after the change (step 5).",
        ),
    )
    private val faultInThisApp = panel(
        Widgets.resultMark(context, R.drawable.ic_x, filled = true),
        title("The fault is in this app."),
        text("The test event did not arrive. Try these:"),
        Widgets.checklist(
            context,
            "Keep the “Band relay” notification on (step 6).",
            "Close this app in the recent apps screen, then open it again.",
        ),
    )
    private val panels = listOf(watching, arrived, noPress, faultInGadgetbridge, faultInThisApp)

    private val body = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(0, dp(32f), 0, dp(32f))
        for (panel in panels) {
            addView(panel, LinearLayout.LayoutParams(MATCH, WRAP))
        }
    }

    private val cancel = Widgets.button(context, "Cancel") { navigator.back() }
    private val done = Widgets.primaryButton(context, "Done") { navigator.back() }
    private val sendTest = Widgets.primaryButton(context, "Send a test event") { happen(Happening.SendTest) }
    private val sendAgain = Widgets.button(context, "Send the test again") { happen(Happening.SendTest) }
    private val backToSetup = Widgets.primaryButton(context, "Back to setup steps") { navigator.back() }
    private val tryAgain = Widgets.button(context, "Try the band again") { happen(Happening.TryBandAgain) }

    /**
     * Every button of the five frames, in one order that serves them all:
     * frame 14 puts "Back to setup steps" over "Try the band again", and
     * frame 15 puts it under "Send the test again". [show] shows a state's.
     */
    private val buttons = listOf(cancel, done.view, sendTest.view, sendAgain, backToSetup.view, tryAgain)
    private val footer = Widgets.footer(context, *buttons.toTypedArray())

    init {
        show(state)
    }

    override fun body(): View = body

    override fun footer(): View = footer

    /**
     * D16: the check is the visible top. One that was watching or waiting
     * for its test starts a full watch again, since the capture was let go
     * while it was away, and the watch counts from now. A result stays as
     * it was.
     */
    override fun enter() = move(if (state.listening) CheckState.Watching else state)

    /** D16: the check is no longer the visible top, so it lets go of the capture and stops its timer. */
    override fun leave() = letGo()

    /** What [happened], as [next] decides it. A pair that changes nothing leaves the timer and the capture as they are. */
    private fun happen(happened: Happening) {
        val after = next(state, happened)
        if (after != state) {
            move(after)
        }
    }

    /**
     * Makes [to] the state. The old state's capture and timer are let go
     * first, whatever it was, and only a listening state takes them again,
     * so no way through the check can leave the capture armed. A new state
     * is drawn, and starts the navigator's quiet time.
     */
    private fun move(to: CheckState) {
        letGo()
        val changed = to != state
        state = to
        if (changed) {
            show(to)
            navigator.changedInPlace()
        }
        if (to.listening) {
            listen(to)
        }
    }

    /**
     * Arms the capture for every event, and starts the state's timer: the
     * watch's [LISTEN_SECONDS], one second at a time for the countdown, or
     * the test's [TEST_EVENT_WAIT_MS]. The test event goes out after the
     * capture is armed, unless a debug fixture loses it (frame 15). Only
     * SendTest leads to TestSent, since a check entered again watches.
     */
    private fun listen(to: CheckState) {
        Capture.arm(Capture.Wants.AnyEvent) { payload, test ->
            happen(Happening.Heard(payload, System.currentTimeMillis(), test))
        }
        holding = true
        when (to) {
            CheckState.Watching -> {
                secondsLeft = LISTEN_SECONDS
                watchStartedAt = SystemClock.uptimeMillis()
                countdown.show(secondsLeft)
                scheduleSecond()
            }
            CheckState.TestSent -> {
                clock.postAtTime(testWaitOver, SystemClock.uptimeMillis() + TEST_EVENT_WAIT_MS)
                if (!Fixtures.dropTestEvent(context)) {
                    BandReceiver.sendTestEvent(context)
                }
            }
            else -> Unit
        }
    }

    /** Stops the timers, and lets go of the capture if this page holds it. */
    private fun letGo() {
        clock.removeCallbacks(nextSecond)
        clock.removeCallbacks(testWaitOver)
        if (holding) {
            holding = false
            Capture.disarm()
        }
    }

    /** One second of the watch: the countdown moves on, and the last one ends the watch. */
    private fun second() {
        secondsLeft -= 1
        if (secondsLeft > 0) {
            countdown.show(secondsLeft)
            scheduleSecond()
        } else {
            happen(Happening.TimedOut)
        }
    }

    /** Each second is timed from the start of the watch, so a slow redraw does not stretch the ten. */
    private fun scheduleSecond() {
        val gone = LISTEN_SECONDS - secondsLeft + 1
        clock.postAtTime(nextSecond, watchStartedAt + gone * 1000L)
    }

    /**
     * Draws [state]: its frame's panel and its frame's buttons. TestSent is
     * frame 13 with "Send a test event" disabled while the test is out
     * (D10). Frame 12 names the event that arrived and when.
     */
    private fun show(state: CheckState) {
        val panel = when (state) {
            CheckState.Watching -> watching
            is CheckState.Arrived -> arrived
            CheckState.NoPress, CheckState.TestSent -> noPress
            CheckState.FaultInGadgetbridge -> faultInGadgetbridge
            CheckState.FaultInThisApp -> faultInThisApp
        }
        val shown = when (state) {
            CheckState.Watching -> listOf(cancel)
            is CheckState.Arrived -> listOf(done.view)
            CheckState.NoPress, CheckState.TestSent -> listOf(sendTest.view, tryAgain)
            CheckState.FaultInGadgetbridge -> listOf(backToSetup.view, tryAgain)
            CheckState.FaultInThisApp -> listOf(sendAgain, backToSetup.view)
        }
        if (state is CheckState.Arrived) {
            arrivedLine.text = Words.arrived(state.label, state.at)
        }
        sendTest.enabled = state != CheckState.TestSent
        for (view in panels) {
            Widgets.reveal(view, view === panel)
        }
        for (view in buttons) {
            view.isVisible = view in shown
        }
    }

    /** One frame's content: its children one under another, 16 dp apart, as the frames' HTML lays them out. */
    private fun panel(vararg children: View): LinearLayout = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        showDividers = LinearLayout.SHOW_DIVIDER_MIDDLE
        dividerDrawable = GradientDrawable().apply {
            val size = dp(16f)
            setSize(size, size)
        }
        for (child in children) {
            addView(child)
        }
    }

    /**
     * A state's title: Inter 24 semibold, on 30 sp lines. It is live, so a
     * screen reader reads it when its state takes the place of another.
     */
    private fun title(words: String): TextView = Widgets.text(context, words, 24f, 600, lineHeightSp = 30f, live = true)

    /** A result's text: Inter 15, on 22 sp lines. */
    private fun text(words: String): TextView = Widgets.text(context, words, 15f, lineHeightSp = 22f)

    /** The design's "Gap": room between the text and the countdown, on top of the panel's own gaps (frame 11). */
    private fun space(heightDp: Float): View = View(context).apply {
        layoutParams = LinearLayout.LayoutParams(MATCH, dp(heightDp))
    }

    private fun dp(value: Float): Int = Ink.dp(context, value)
}
