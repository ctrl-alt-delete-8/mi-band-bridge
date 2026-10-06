package dev.bandbridge.relay.ui

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import android.util.Log
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import androidx.core.view.isVisible
import dev.bandbridge.relay.Event
import dev.bandbridge.relay.Events
import dev.bandbridge.relay.Outcome
import dev.bandbridge.relay.R
import dev.bandbridge.relay.Rule
import dev.bandbridge.relay.Rules
import dev.bandbridge.relay.RulesFile
import dev.bandbridge.relay.Words
import dev.bandbridge.relay.words
import java.io.IOException
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

private const val MATCH = ViewGroup.LayoutParams.MATCH_PARENT
private const val WRAP = ViewGroup.LayoutParams.WRAP_CONTENT

/** An event's time, as its row prints it: 09:41:08. */
private val TIME: DateTimeFormatter = DateTimeFormatter.ofPattern("HH:mm:ss", Locale.US)

/**
 * The request codes of Android's Save and Open dialogs, which the window
 * hands back to [MainScreen.fileChosen]. They start at 2, past the window's
 * own 1, its ask for the notification permission.
 */
private const val SAVE_RULES = 2
private const val OPEN_RULES = 3

/** The page's tag in the log. */
private const val TAG = "MainScreen"

/**
 * The main page (frames 02, 03, 04, 09, 16 and 17): the last band events,
 * the rules with their ⋮ menu, and the quiet Setup row at the foot.
 *
 * On a phone the events come first and the rules under them, in one scroll.
 * A tablet sets the two columns side by side (D3) and keeps the events live
 * beside an open editor, Setup or check. Both columns are drawn from storage
 * by [refresh], which the navigator calls when a band event arrives, when the
 * app comes back, and when a page over this one closes. A column whose data
 * has not changed is left as it is, so an e-ink panel does not redraw it for
 * nothing.
 *
 * The ⋮ menu exports the rules to a file and imports them from one, through
 * Android's own Save and Open dialogs (the note, section 8). What came of it
 * shows as a line under the Rules header until the next rules action (B6).
 */
class MainScreen(private val context: Context, private val navigator: Navigator) : SplitPage {
    override val title = "Mi Band Bridge"
    override val status = "Listening"

    private val events = column()

    /**
     * Built once, and moved into each new Rules header, so that an open
     * menu keeps its anchor when the rules are drawn again under it.
     */
    private val addRule: View = Widgets.button(context, "Add a rule", R.drawable.ic_plus) { editRule(at = null, prefill = null) }
    private val menuButton: View = Widgets.iconButton(context, R.drawable.ic_ellipsis_vertical, "Rules menu") { openMenu() }

    /**
     * The result line (frame 17), in place of the hint while it shows.
     *
     * Its words are live, so a screen reader reads a result that appears
     * while the window is in front, such as that no app answers. A result
     * from a file comes while Android's dialog still covers the window, and
     * Android reports no change then. A screen reader reads it with the page
     * when the window takes the focus again.
     */
    private val resultWords = Widgets.text(context, "", 14f, lineHeightSp = 20f, live = true)
    private val result: View = Widgets.notice(context, resultWords, insetDp = 10f)

    private val hint: View = Widgets.text(context, "Tap a rule to change it.", 13f, color = Ink.DIM, lineHeightSp = 19f)
    private val noRules: View = emptyLine("No rules yet. A rule sends a Key Mapper key map when the band sends a signal.")
    private val ruleRows = column()

    /**
     * The Rules header, the result line or the hint under it, the line that
     * says there are no rules, and the rows. Its bottom padding ends the
     * phone's scroll as frame 08 ends a long page.
     */
    private val rules = column().apply {
        setPadding(0, 0, 0, dp(32f))
        addView(result, LinearLayout.LayoutParams(MATCH, WRAP))
        addView(hint, LinearLayout.LayoutParams(MATCH, WRAP))
        addView(noRules, LinearLayout.LayoutParams(MATCH, WRAP))
        addView(ruleRows, LinearLayout.LayoutParams(MATCH, WRAP))
    }
    private var rulesHeading: View? = null

    private val stacked: View = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        addView(events, LinearLayout.LayoutParams(MATCH, WRAP))
        addView(rules, LinearLayout.LayoutParams(MATCH, WRAP))
    }

    private val setupRow: View = Widgets.quietRow(context, R.drawable.ic_settings, "Setup") {
        navigator.show(Screen.Setup(asRoot = false))
    }

    /** What each column shows now, so a refresh that finds nothing new draws nothing. */
    private var shownEvents: List<Event>? = null
    private var shownRules: List<Rule>? = null

    /**
     * The result line under the Rules header, or null for the hint (B6). It
     * is not stored: a turn of the screen or a visit to Setup keeps it, as
     * this page stays, but a new process starts with the hint.
     */
    private var line: String? = null

    init {
        Widgets.reveal(result, false)
        refresh()
    }

    override fun body(): View = stacked

    override fun eventsColumn(): View = events

    override fun rulesColumn(): View = rules

    override fun footer(): View = setupRow

    override fun refresh() {
        showEvents(Events.all(context))
        showRules(Rules.load(context))
    }

    /**
     * The ⋮ menu of frame 16: Export rules, disabled while there are no
     * rules (B2), and Import rules. Opening it is the next rules action, so
     * the line of the last one goes (B6).
     */
    internal fun openMenu() {
        showLine(null)
        val any = Rules.load(context).isNotEmpty()
        Widgets.menu(
            navigator.host,
            menuButton,
            listOf(
                MenuItem("Export rules", R.drawable.ic_download, enabled = any) { exportRules() },
                MenuItem("Import rules", R.drawable.ic_upload) { importRules() },
            ),
        )
    }

    /**
     * What Android's Save or Open dialog answered, as the window hands it
     * over: the [request] this page made, and the [file] the person chose.
     * A dialog left without a choice never gets here, and changes nothing.
     */
    fun fileChosen(request: Int, file: Uri) {
        when (request) {
            SAVE_RULES -> saveTo(file)
            OPEN_RULES -> openFrom(file)
        }
    }

    /**
     * Shows [words] as the line under the Rules header (B6), with the name
     * of the [file] they end with on a line of its own, as frame 17 breaks
     * it. The rules are drawn again with it, since an import may have added
     * some. The page changed with no tap on it, so taps wait a moment, as
     * after any change under the finger.
     */
    internal fun showResult(words: String, file: String? = null) {
        showLine(if (file == null) words else Words.fileOnItsOwnLine(words, file))
        showRules(Rules.load(context))
        navigator.changedInPlace()
    }

    /**
     * The events, newest first, or the line that says there are none (frame
     * 04). Clear shows only when there is something to clear.
     */
    private fun showEvents(list: List<Event>) {
        if (list == shownEvents) {
            return
        }
        shownEvents = list
        events.removeAllViews()
        val clear = if (list.isEmpty()) null else Widgets.button(context, "Clear") { clear() }
        events.addView(Widgets.listHeading(context, "Events", action = clear))
        if (list.isEmpty()) {
            events.addView(
                emptyLine("Nothing yet. Press a button in the band app."),
            )
        }
        for (event in list) {
            events.addView(eventRow(event))
        }
        // On a phone this is the room between the two lists: 12 dp under the
        // last row (frame 02), 16 dp under the line that says there is none
        // (frame 04). On a tablet it is room at the end of the column. The
        // navigator owns the left and right padding, so they stay as they are.
        events.setPadding(events.paddingLeft, 0, events.paddingRight, dp(if (list.isEmpty()) 16f else 12f))
    }

    /**
     * One event as its row says it (ALhwn). A press that no rule has offers
     * a rule for its signal. Other traffic shows its raw bytes, which wrap
     * anywhere, since hex has no spaces to break at.
     */
    private fun eventRow(event: Event): View {
        val outcome = event.outcome()
        val time = TIME.format(Instant.ofEpochMilli(event.at).atZone(ZoneId.systemDefault()))
        val signal = event.signal
        if (outcome == Outcome.NoRule && signal != null) {
            return Widgets.eventRow(
                context, event.label, outcome.words(), time,
                detail = Widgets.text(context, "Add a rule for $signal", 13f, 600),
                onTap = { editRule(at = null, prefill = signal) },
            )
        }
        val bytes = if (outcome == Outcome.NotBandSignal && event.payload.isNotBlank()) {
            Widgets.text(context, event.payload, 13f, color = Ink.DIM, mono = true)
        } else {
            null
        }
        return Widgets.eventRow(context, event.label, outcome.words(), time, detail = bytes)
    }

    /**
     * The rules in the person's order, or the line that says there are none
     * (frame 04), which has no count and no hint. Add a rule lives here and
     * nowhere else in the app (the note, section 2), beside the ⋮ button,
     * in every state.
     */
    private fun showRules(list: List<Rule>) {
        if (list == shownRules) {
            return
        }
        shownRules = list
        detach(addRule)
        detach(menuButton)
        val count = if (list.isEmpty()) null else list.size.toString()
        val heading = Widgets.listHeading(context, "Rules", count, addRule, menu = menuButton)
        rulesHeading?.let(rules::removeView)
        rules.addView(heading, 0, LinearLayout.LayoutParams(MATCH, WRAP))
        rulesHeading = heading
        noRules.isVisible = list.isEmpty()
        ruleRows.removeAllViews()
        list.forEachIndexed { index, rule ->
            ruleRows.addView(
                Widgets.ruleRow(
                    context, rule.signal, rule.name, rule.keyMapUid,
                    onEdit = { editRule(at = index, prefill = null) },
                    onRemove = { askToRemove(index, rule) },
                ),
            )
        }
        showHint()
    }

    /**
     * The result line under the Rules header, or none (B6). With no rules
     * it shows in the same place, above the line that says there are none.
     * Its words wait while it is hidden and appear with it, so each new
     * result is a change of words, which is what a screen reader reads.
     */
    private fun showLine(words: String?) {
        line = words
        if (words != null) {
            Widgets.setWords(resultWords, words)
        }
        Widgets.reveal(result, words != null)
        showHint()
    }

    /** The hint shows while there are rules and no result line takes its place (frame 17). */
    private fun showHint() {
        hint.isVisible = !shownRules.isNullOrEmpty() && line == null
    }

    /**
     * Opens the rule at [at] in the editor, or a new rule with the signal
     * [prefill]. Add a rule, a tap on a rule, and "Add a rule for bb03" are
     * each a rules action, so the result line of the last one goes (B6).
     */
    private fun editRule(at: Int?, prefill: String?) {
        showLine(null)
        navigator.show(Screen.RuleEditor(at = at, prefill = prefill))
    }

    /**
     * Frame 03. Only Remove removes, and nothing can undo it, at the
     * person's request (the note, section 2). Cancel and Back close the
     * dialog and change nothing. × is a rules action, so the result line
     * goes (B6).
     */
    private fun askToRemove(index: Int, rule: Rule) {
        showLine(null)
        Widgets.confirmDialog(
            navigator.host,
            title = "Remove this rule?",
            chip = rule.signal,
            name = rule.name,
            text = "${rule.signal} will no longer trigger ${rule.name}. You cannot undo this.",
            confirm = "Remove",
        ) { remove(index, rule) }
    }

    /**
     * Removes [rule], which the dialog showed at [index]. Nothing else writes
     * the rules while the dialog covers the page, but the rule is checked
     * again before it goes, so a stale index can never remove another rule.
     */
    private fun remove(index: Int, rule: Rule) {
        val stored = Rules.load(context)
        val at = if (stored.getOrNull(index) == rule) index else stored.indexOf(rule)
        if (at >= 0) {
            Rules.save(context, stored.filterIndexed { i, _ -> i != at })
        }
        navigator.refresh()
    }

    /** Empties the list at once. Setup's last band press line stays, since it is kept apart (D12). */
    private fun clear() {
        Events.clear(context)
        navigator.refresh()
    }

    /**
     * B2: Android's Save dialog, with today's name for the file. The rules
     * are written when it answers, in [saveTo], so they are the ones stored
     * then.
     */
    private fun exportRules() {
        val intent = Intent(Intent.ACTION_CREATE_DOCUMENT)
            .addCategory(Intent.CATEGORY_OPENABLE)
            .setType("application/json")
            .putExtra(Intent.EXTRA_TITLE, RulesFile.fileName(LocalDate.now()))
        ask(intent, SAVE_RULES, Words.NO_APP_TO_SAVE)
    }

    /**
     * B3: Android's Open dialog, for a file of any type. A browser or a
     * messenger may have saved a rules file under another one, so the
     * content decides, in [openFrom].
     */
    private fun importRules() {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT)
            .addCategory(Intent.CATEGORY_OPENABLE)
            .setType(RulesFile.OPEN_TYPE)
        ask(intent, OPEN_RULES, Words.NO_APP_TO_OPEN)
    }

    /**
     * Starts [intent] from the window, whose answer comes back to
     * [fileChosen]. An e-ink reader may have no app that answers the Save or
     * Open dialog, and then the line says so rather than the app stopping
     * (B5). The page lives in MainActivity's window, the one Activity.
     */
    private fun ask(intent: Intent, request: Int, noApp: String) {
        try {
            (context as Activity).startActivityForResult(intent, request)
        } catch (e: ActivityNotFoundException) {
            showResult(noApp)
        }
    }

    /**
     * Writes the stored rules into [file], which Android's Save dialog made
     * (B2), and says how many went there, or that none did.
     *
     * The file lives in another app's storage, which can fail in ways of
     * its own: a full disk, a grant that is gone, a crash in its process.
     * To the person each means the same, that nothing was exported, so each
     * gets the same line. The mode "wt" empties a file that already exists,
     * so a longer old file cannot leave its end behind the new rules.
     */
    private fun saveTo(file: Uri) {
        val rules = Rules.load(context)
        val text = RulesFile.write(rules, Instant.now())
        val written = try {
            val stream = context.contentResolver.openOutputStream(file, "wt")
            stream?.use { it.write(text.toByteArray()) }
            stream != null
        } catch (e: IOException) {
            Log.w(TAG, "Could not write the rules file", e)
            false
        } catch (e: RuntimeException) {
            Log.w(TAG, "Could not write the rules file", e)
            false
        }
        if (written) {
            val name = nameOf(file)
            showResult(Words.exported(rules.size, name), name)
        } else {
            showResult(Words.NOT_WRITTEN)
        }
    }

    /**
     * Reads [file], which Android's Open dialog gave, and adds the rules in
     * it that are new, at the end of the list, in the file's order (B4).
     * Import only adds: a rule already there never changes. A file that is
     * not a rules file, or that cannot be read, adds nothing, and the line
     * says which. Reading happens here, on the window's thread: a rules file
     * is small. [RulesFile.MAX_BYTES] bounds what is read, and
     * [RulesFile.MAX_RULES] how long the list can grow.
     */
    private fun openFrom(file: Uri) {
        val read = try {
            context.contentResolver.openInputStream(file)?.use { RulesFile.read(it) }
        } catch (e: IOException) {
            Log.w(TAG, "Could not read the rules file", e)
            null
        } catch (e: RuntimeException) {
            Log.w(TAG, "Could not read the rules file", e)
            null
        }
        when (read) {
            null -> showResult(Words.NOT_READ)
            RulesFile.Read.NotARulesFile -> showResult(Words.NOT_A_RULES_FILE)
            is RulesFile.Read.Rules -> {
                val stored = Rules.load(context)
                val merged = RulesFile.merged(stored, read.rules)
                if (merged.size > stored.size) {
                    Rules.save(context, merged)
                }
                val name = nameOf(file)
                showResult(Words.imported(merged.size - stored.size, name), name)
            }
        }
    }

    /**
     * The name that Android's Save or Open dialog shows for [file], which
     * the line gives (B5). A provider that will not say gives the last part
     * of the file's address instead.
     */
    private fun nameOf(file: Uri): String {
        val name = try {
            context.contentResolver.query(file, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
                if (cursor.moveToFirst()) cursor.getString(0) else null
            }
        } catch (e: RuntimeException) {
            null
        }
        return name?.takeIf { it.isNotBlank() } ?: file.lastPathSegment ?: file.toString()
    }

    /** The gray line that stands in for an empty list (frame 04). */
    private fun emptyLine(words: String): View =
        Widgets.text(context, words, 15f, color = Ink.DIM, lineHeightSp = 22f)

    private fun column(): LinearLayout = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }

    private fun detach(view: View) {
        (view.parent as? ViewGroup)?.removeView(view)
    }

    private fun dp(value: Float): Int = Ink.dp(context, value)
}
