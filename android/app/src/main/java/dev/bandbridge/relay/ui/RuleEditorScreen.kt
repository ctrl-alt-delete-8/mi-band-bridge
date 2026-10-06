package dev.bandbridge.relay.ui

import android.content.Context
import android.graphics.drawable.Drawable
import android.graphics.drawable.GradientDrawable
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.text.Editable
import android.text.TextWatcher
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import androidx.core.view.isVisible
import dev.bandbridge.relay.LISTEN_SECONDS
import dev.bandbridge.relay.Learn
import dev.bandbridge.relay.R
import dev.bandbridge.relay.Rule
import dev.bandbridge.relay.Rules
import dev.bandbridge.relay.RulesFile
import dev.bandbridge.relay.Signals
import dev.bandbridge.relay.Words

private const val MATCH = ViewGroup.LayoutParams.MATCH_PARENT
private const val WRAP = ViewGroup.LayoutParams.WRAP_CONTENT

private const val LEARN = "Learn from the band"
private const val TRY_AGAIN = "Try again"

/**
 * The rule editor (frames 05, 06, 07 and 10): a rule's signal, name and key
 * map id, and Save.
 *
 * [at] is the rule's place in the list, or null for a new rule, whose signal
 * [prefill] fills in, as "Add a rule for bb03" does. Save stores the rule and
 * goes back. Back leaves without saving, and the editor has no other button
 * (the note, section 1). Save is disabled while the line above it says what
 * is missing (D11).
 *
 * Learn from the band listens for one press and fills in its signal, with no
 * key map run (D9). It is the one place a press is swallowed on purpose, so
 * every way out of it lets go of the capture: the press, Cancel, the time
 * running out, and [leave], which Back, a page over the editor and the app
 * going to the background all reach (D16).
 */
class RuleEditorScreen(
    private val context: Context,
    private val navigator: Navigator,
    private val at: Int?,
    prefill: String?,
) : Page {
    /** Where Learn is: the plain signal row (frame 05), listening (06), or after no press arrived (07). */
    private enum class Mode { Plain, Listening, NoPress }

    /** The rule as it was opened. Null for a new rule, and for one no longer in the list, which Save adds again. */
    private val original: Rule? = at?.let { Rules.load(context).getOrNull(it) }

    override val title = if (original != null) "Edit rule" else "New rule"

    private val learn = Learn(::learnEnded)

    /** Calls [second] once a second while Learn listens, on the main thread. */
    private val clock = Handler(Looper.getMainLooper())
    private val nextSecond = Runnable { second() }
    private var roundStartedAt = 0L

    private val learnButton = Widgets.button(context, LEARN, R.drawable.ic_radio) { startLearning() }

    // Each field takes at most what a rules file may hold, so a paste cannot
    // store what an import refuses: one field of a million characters stops
    // the app from starting (RulesFile.MAX_FIELD_LENGTH).
    private val signal = Widgets.field(
        context, "Signal", "—", mono = true, beside = learnButton,
        maxLength = RulesFile.MAX_FIELD_LENGTH,
    )
    private val name = Widgets.field(
        context, "Name", "For example, Page down", mono = false,
        help = "A name for this rule, shown in the rule list.",
        maxLength = RulesFile.MAX_FIELD_LENGTH,
    )
    private val keyMapUid = Widgets.field(
        context, "Key map id", "Paste the id from Key Mapper", mono = true,
        help = "Copy it from the key map’s Options tab in Key Mapper.",
        maxLength = RulesFile.MAX_FIELD_LENGTH,
    )

    /** The signal input and Learn, which frame 06's box takes the place of. */
    private val signalRow = signal.input.parent as View

    private val countdown = Widgets.countdown(context)
    private val listeningBox = Widgets.listeningBox(
        context,
        title = "Waiting for a press…",
        text = "Press the button in the band app that you want to use.",
        countdown = countdown,
    ) { cancelLearning() }

    private val noPress = Widgets.notice(
        context,
        Widgets.text(context, "No press arrived in $LISTEN_SECONDS seconds.", 15f, 600, live = true),
        Widgets.text(context, "Open setup →", 15f, 600),
        onTap = { navigator.show(Screen.Setup(asRoot = false)) },
    )

    /** Pushes the note to the foot of the page, as frame 05 places it. */
    private val spacer = View(context)
    private val note = Widgets.note(context, "One signal can be in more than one rule. Every matching key map is sent.")

    private val body = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        showDividers = LinearLayout.SHOW_DIVIDER_MIDDLE
        addView(
            LinearLayout(context).apply {
                orientation = LinearLayout.VERTICAL
                showDividers = LinearLayout.SHOW_DIVIDER_MIDDLE
                dividerDrawable = gap(10f)
                addView(signal.view, LinearLayout.LayoutParams(MATCH, WRAP))
                addView(noPress, LinearLayout.LayoutParams(MATCH, WRAP))
            },
            LinearLayout.LayoutParams(MATCH, WRAP),
        )
        addView(name.view, LinearLayout.LayoutParams(MATCH, WRAP))
        addView(keyMapUid.view, LinearLayout.LayoutParams(MATCH, WRAP))
        addView(spacer, LinearLayout.LayoutParams(MATCH, 0, 1f))
        addView(note, LinearLayout.LayoutParams(MATCH, WRAP))
    }

    /** What Save is waiting for, in the words of [Words.missing]. */
    private val missing = Widgets.text(context, "", 13f, color = Ink.DIM)
    private val save = Widgets.primaryButton(context, "Save") { save() }
    private val footer = Widgets.footer(context, missing, save.view, gapDp = 8f)

    private var mode = Mode.Plain
    private var inColumn = false

    /** The line above Save as last drawn, so a key that changes nothing redraws nothing. */
    private var shownLine: String? = null
    private var lineDrawn = false

    init {
        // Frame 06's box goes under the signal's label with the row, and [show] shows one of the two.
        val signalColumn = signal.view as ViewGroup
        signalColumn.addView(listeningBox, signalColumn.indexOfChild(signalRow) + 1, LinearLayout.LayoutParams(MATCH, WRAP))
        if (original != null) {
            signal.input.setText(original.signal)
            name.input.setText(original.name)
            keyMapUid.input.setText(original.keyMapUid)
        } else if (prefill != null) {
            signal.input.setText(prefill)
        }
        val typed = object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) = Unit
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) = Unit
            override fun afterTextChanged(s: Editable?) = showMissing()
        }
        for (field in listOf(signal, name, keyMapUid)) {
            field.input.addTextChangedListener(typed)
        }
        showMissing()
        place(inColumn = false)
    }

    override fun body(): View = body

    override fun footer(): View = footer

    /**
     * Frame 10 on a tablet: 20 dp between the fields and no note, where
     * frame 05 on a phone has 24 dp and the note at the foot. The name's help
     * follows [show].
     */
    override fun place(inColumn: Boolean) {
        this.inColumn = inColumn
        body.dividerDrawable = gap(if (inColumn) 20f else 24f)
        body.setPadding(body.paddingLeft, dp(if (inColumn) 4f else 24f), body.paddingRight, dp(if (inColumn) 16f else 24f))
        spacer.isVisible = !inColumn
        note.isVisible = !inColumn
        show(mode)
    }

    /** D16: the editor is no longer the visible top, so Learn stops and lets go of the capture. */
    override fun leave() {
        if (learn.listening) {
            cancelLearning()
        }
    }

    /**
     * Shows [mode]: the signal row or frame 06's box in its place, frame 07's
     * notice under the row, and the Learn button's words. The box and the
     * notice have live titles, which a screen reader reads as they appear.
     *
     * The name's help shows only with the plain row on a phone. Frames 06
     * and 07 draw none while the box or the notice speaks, and frame 10 none
     * in a tablet's column.
     */
    private fun show(mode: Mode) {
        this.mode = mode
        signalRow.isVisible = mode != Mode.Listening
        Widgets.reveal(listeningBox, mode == Mode.Listening)
        Widgets.reveal(noPress, mode == Mode.NoPress)
        val words = if (mode == Mode.NoPress) TRY_AGAIN else LEARN
        if (learnButton.text.toString() != words) {
            learnButton.text = words
        }
        name.help?.isVisible = mode == Mode.Plain && !inColumn
    }

    /**
     * Learn from the band, or Try again: a new round of [LISTEN_SECONDS], one
     * second at a time.
     *
     * The keyboard closes, and the fields give up the focus. The round wants
     * a band button, and the box hides the signal input, so a keyboard opened
     * for it would type into nothing and cover half the page. And a field that
     * kept the focus, such as the name just typed, would blink its cursor
     * through the round and after it, which on e-ink redraws the panel.
     */
    private fun startLearning() {
        learn.start()
        roundStartedAt = SystemClock.uptimeMillis()
        countdown.show(learn.secondsLeft)
        show(Mode.Listening)
        navigator.takeFocus()
        clock.removeCallbacks(nextSecond)
        scheduleSecond()
    }

    /** Each second is timed from the start of the round, so a slow redraw does not stretch the ten. */
    private fun scheduleSecond() {
        val gone = LISTEN_SECONDS - learn.secondsLeft + 1
        clock.postAtTime(nextSecond, roundStartedAt + gone * 1000L)
    }

    /** One second of the round: the countdown moves on, and the last one shows frame 07 through [learnEnded]. */
    private fun second() {
        learn.tick()
        if (learn.listening) {
            countdown.show(learn.secondsLeft)
            scheduleSecond()
        }
    }

    /** Cancel, or the editor left while it listened: the row comes back as it was. */
    private fun cancelLearning() {
        clock.removeCallbacks(nextSecond)
        learn.stop()
        show(Mode.Plain)
    }

    /**
     * A press fills in its signal, and the row comes back. Ten seconds with
     * none show frame 07.
     *
     * Either way the form changes under the finger with no tap to cause it,
     * so the navigator ignores taps for a moment. Frame 07's notice, one tap
     * target that opens Setup, lies where Cancel was, and a tap meant for
     * Cancel as the time ran out would otherwise open Setup.
     */
    private fun learnEnded(end: Learn.End) {
        clock.removeCallbacks(nextSecond)
        when (end) {
            is Learn.End.Heard -> {
                signal.input.setText(end.signal)
                show(Mode.Plain)
            }
            Learn.End.NoPress -> show(Mode.NoPress)
        }
        navigator.changedInPlace()
    }

    /** The line above Save, and Save enabled only when there is none. */
    private fun showMissing() {
        val line = Words.missing(signal.input.text.toString(), name.input.text.toString(), keyMapUid.input.text.toString())
        if (lineDrawn && line == shownLine) {
            return
        }
        lineDrawn = true
        shownLine = line
        missing.text = line.orEmpty()
        missing.isVisible = line != null
        save.enabled = line == null
    }

    /**
     * Stores the signal normalized, lower case hex as the Mac writes it, and
     * the name and id trimmed. A new rule goes last and an edited one keeps
     * its place. Then back, to the list that shows it.
     */
    private fun save() {
        val normalized = Signals.normalize(signal.input.text.toString()) ?: return
        val rule = Rule(normalized, name.input.text.toString().trim(), keyMapUid.input.text.toString().trim())
        Rules.save(context, Rules.saved(Rules.load(context), rule, was = original, at = at))
        navigator.back()
    }

    /** Room between the children of a column, as the design's gap. */
    private fun gap(gapDp: Float): Drawable = GradientDrawable().apply {
        val size = dp(gapDp)
        setSize(size, size)
    }

    private fun dp(value: Float): Int = Ink.dp(context, value)
}
