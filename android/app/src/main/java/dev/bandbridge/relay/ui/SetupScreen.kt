package dev.bandbridge.relay.ui

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.graphics.Paint
import android.graphics.drawable.GradientDrawable
import android.net.Uri
import android.util.Log
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import dev.bandbridge.relay.About
import dev.bandbridge.relay.Events
import dev.bandbridge.relay.Gate
import dev.bandbridge.relay.RequiredApp
import dev.bandbridge.relay.Words

private const val MATCH = ViewGroup.LayoutParams.MATCH_PARENT

private const val TAG = "Setup"

/**
 * The characteristic Gadgetbridge is to pass on: the band's chunked read
 * characteristic, which carries the band app's messages (docs/PROTOCOL.md).
 * Without this filter every band notification would reach the relay.
 */
private const val BLE_CHARACTERISTICS = "00000017-0000-3512-2118-0009af100700"

/** Where a missing app can be had: its F-Droid page, and its own releases. Never Google Play (the note, section 6). */
private const val GADGETBRIDGE_FDROID = "https://f-droid.org/packages/nodomain.freeyourgadget.gadgetbridge"
private const val GADGETBRIDGE_CODEBERG = "https://codeberg.org/Freeyourgadget/Gadgetbridge/releases"
private const val KEY_MAPPER_FDROID = "https://f-droid.org/packages/io.github.sds100.keymapper"
private const val KEY_MAPPER_GITHUB = "https://github.com/keymapperorg/KeyMapper/releases"

/**
 * Setup (frames 01 and 08): whether the two required apps are installed, the
 * steps that connect Gadgetbridge to this app, and how to find a key map id.
 *
 * As the gate's root it opens with the intro, says "Not set up", and has no
 * Back, since the app cannot work yet: the window puts Main in its place once
 * both apps are there (the note, section 1). Reached from Main's Setup row it
 * has Back and no intro. Both show the same steps, with Key Mapper's note for
 * 4.4 or older, whatever version is installed (D14). The design draws only
 * the top of the root, and the steps below it are frame 08's. Under the
 * required apps, both say that the band needs a custom band app, since the
 * relay hears no other press (A3).
 *
 * Each app shows its own state, so after one install the root lists one
 * check and one ×. A missing app offers its download pages. An installed
 * Gadgetbridge says when the last band press arrived (D12), which [refresh]
 * keeps current as presses come in. The two values Gadgetbridge needs are
 * shown, not typed, each with a copy button (D13).
 *
 * A copy's check mark lasts as long as this page is on the navigator's
 * stack. The person copies one value, pastes it in Gadgetbridge and comes
 * back, and the mark still says which value is done, through the app going
 * to the background and through the check over this page. Back, or the
 * gate putting Main in place of the root, drops the page, and the next
 * Setup starts with no marks.
 */
class SetupScreen(private val context: Context, private val navigator: Navigator, asRoot: Boolean) : Page {
    override val title = "Setup"
    override val status: String? = if (asRoot) "Not set up" else null
    override val canGoBack = !asRoot

    /** The two required apps' rows, built again by [refresh] when an app comes or goes. */
    private val apps = column(8f)

    /** Gadgetbridge's row while it is installed, whose line says when the last band press arrived (D12). */
    private var lastPressRow: StatusRow? = null

    /** Step 4's first value: the characteristic Gadgetbridge is to pass on. */
    private val characteristics = Widgets.copyField(context, "BLE Characteristics", BLE_CHARACTERISTICS)

    /**
     * Step 4's second value: this app's package, read rather than written
     * out, so it is always the one the receivers listen as.
     */
    private val apiPackage = Widgets.copyField(context, "BLE API package", context.packageName)

    private val body: View = column(8f).apply {
        setPadding(0, dp(20f), 0, dp(32f))
        if (asRoot) {
            addView(
                Widgets.text(
                    context,
                    "This app needs Gadgetbridge and Key Mapper.",
                    15f, lineHeightSp = 22f,
                ),
            )
            addView(space(8f))
        }
        addView(Widgets.sectionHead(context, "REQUIRED APPS"))
        addView(apps)
        // A3. The relay hears the presses of a custom band app only (A1), so
        // a band without one sends it nothing, and the check cannot pass. The
        // person may not know the desktop app by name, so the notice says
        // where to download it, by the user's words on 2026-09-28. The
        // user asked for each instruction once, in as few words as it needs.
        // The address shows in full, underlined as a link, since an e-ink
        // screen has no color for one, and the whole notice opens it.
        addView(
            Widgets.notice(
                context,
                Widgets.text(context, Words.DESKTOP_APP_NOTICE, 14f, lineHeightSp = 20f),
                Widgets.text(context, About.DOWNLOADS, 14f, weight = 600, lineHeightSp = 20f).apply {
                    paintFlags = paintFlags or Paint.UNDERLINE_TEXT_FLAG
                },
                onTap = { open(About.DOWNLOADS) },
            ),
        )
        // To-do 17: a person without the desktop app installs the sample band
        // app from the same page, with Gadgetbridge.
        addView(
            Widgets.notice(
                context,
                Widgets.text(context, Words.SAMPLE_APP_NOTICE, 14f, lineHeightSp = 20f),
            ),
        )
        addView(space(16f))
        addView(Widgets.sectionHead(context, "CONNECT GADGETBRIDGE"))
        addView(Widgets.step(context, 1, "Connect the band in Gadgetbridge."))
        addView(Widgets.step(context, 2, "In Gadgetbridge, open this device’s settings → Developer options → BLE Intent API."))
        addView(Widgets.step(context, 3, "Switch on “Broadcast GATT notification Intents through BLE Intent API”."))
        addView(
            Widgets.step(
                context, 4,
                "Copy these two values into the matching fields. Do not type them: one extra space stops all events.",
                characteristics.view, apiPackage.view,
            ),
        )
        // The user's words, not frame 08's (B7). Quit in Gadgetbridge's side
        // menu stops its service and ends its process, so it reads the two
        // values again when it is opened. A swipe in the recent apps screen
        // closes only its window, and the service runs on with the old ones.
        addView(
            Widgets.step(
                context, 5,
                "In Gadgetbridge, open the side menu, tap Quit, then open Gadgetbridge again.",
            ),
        )
        addView(Widgets.step(context, 6, "Do not dismiss the “Band relay” notification."))
        addView(
            Widgets.step(
                context, 7,
                "Check that band presses arrive.",
                Widgets.primaryButton(context, "Start check") { navigator.show(Screen.Check) }.view,
                rule = false,
            ),
        )
        addView(space(16f))
        addView(Widgets.sectionHead(context, "FIND A KEY MAP ID"))
        addView(
            Widgets.text(
                context,
                "Each rule needs the ID of a Key Mapper key map.",
                14f, color = Ink.DIM, lineHeightSp = 20f,
            ),
        )
        addView(Widgets.step(context, 1, "In Key Mapper, open the key map that you want the band to run."))
        addView(Widgets.step(context, 2, "Open the Options tab. On some screens, it is in the “Constraints and more” tab."))
        addView(Widgets.step(context, 3, "Under Information, tap the copy icon next to Key map ID."))
        addView(Widgets.step(context, 4, "In this app, add a rule and paste the ID into Key map id."))
        addView(
            Widgets.notice(
                context,
                Widgets.text(
                    context,
                    "Key Mapper 4.4 or older: first turn on “Allow other apps to control this key map with intents or shortcuts” in the same tab.",
                    14f, lineHeightSp = 20f,
                ),
            ),
        )
        addView(space(16f))
        addView(Widgets.sectionHead(context, "ABOUT"))
        addView(Widgets.text(context, About.name(versionName()), 15f, lineHeightSp = 22f))
        addView(Widgets.text(context, About.UNOFFICIAL, 14f, color = Ink.DIM, lineHeightSp = 20f))
        addView(Widgets.text(context, About.LICENSES, 14f, color = Ink.DIM, lineHeightSp = 20f))
        for ((label, url) in About.links) {
            addView(Widgets.downloadButtons(context, label to { open(url) }))
        }
    }

    /** The missing apps the rows show, so a refresh builds them again only when an app came or went. */
    private var shownMissing: Set<RequiredApp>? = null

    init {
        refresh()
    }

    override fun body(): View = body

    /**
     * Asks the phone again which apps are installed, and reads the last band
     * press. The window calls this when it comes back, which is when an app
     * may have been installed, and when a band event arrives.
     *
     * The rows are built again only when an app came or went. A band press
     * changes the last press line alone, so an e-ink panel redraws that one
     * line, and a refresh that finds nothing new draws nothing.
     */
    override fun refresh() {
        val missing = Gate.missing(context)
        if (missing != shownMissing) {
            shownMissing = missing
            val installed = RequiredApp.Gadgetbridge !in missing
            val gadgetbridge = gadgetbridgeRow(installed)
            lastPressRow = gadgetbridge.takeIf { installed }
            apps.removeAllViews()
            apps.addView(gadgetbridge.view)
            apps.addView(keyMapperRow(installed = RequiredApp.KeyMapper !in missing).view)
        }
        lastPressRow?.text = lastPress()
    }

    /** D12's line: when the last real band event arrived, which Clear does not wipe. */
    private fun lastPress(): String = Words.lastPress(Events.lastArrival(context), System.currentTimeMillis())

    /**
     * Gadgetbridge's row: installed, with the last press line, or missing,
     * with its download pages. Its Codeberg releases also hold a Bangle.js
     * build, which is the wrong file, so the row says which to take.
     */
    private fun gadgetbridgeRow(installed: Boolean): StatusRow = if (installed) {
        Widgets.statusRow(context, ok = true, "Gadgetbridge is installed", lastPress())
    } else {
        Widgets.statusRow(
            context, ok = false, "Gadgetbridge is not installed", "It sends band presses to this app.",
            Widgets.downloadButtons(
                context,
                "F-Droid" to { open(GADGETBRIDGE_FDROID) },
                "Codeberg" to { open(GADGETBRIDGE_CODEBERG) },
            ),
            Widgets.text(
                context,
                "On Codeberg, download the file that starts with “gadgetbridge”, not “banglejs”.",
                13f, color = Ink.DIM, lineHeightSp = 19f,
            ),
        )
    }

    private fun keyMapperRow(installed: Boolean): StatusRow = if (installed) {
        Widgets.statusRow(context, ok = true, "Key Mapper is installed", "Rules can run key maps.")
    } else {
        Widgets.statusRow(
            context, ok = false, "Key Mapper is not installed", "Rules need it to run a key map.",
            Widgets.downloadButtons(
                context,
                "F-Droid" to { open(KEY_MAPPER_FDROID) },
                "GitHub" to { open(KEY_MAPPER_GITHUB) },
            ),
        )
    }

    /**
     * Opens [url] in the browser, or in the F-Droid app, which takes its own
     * links.
     *
     * A phone with neither has nothing to open it with. The design has no
     * words for that, so the page stays as it is rather than stop the app.
     */
    private fun open(url: String) {
        try {
            context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
        } catch (e: ActivityNotFoundException) {
            Log.w(TAG, "Nothing on this phone opens $url")
        }
    }

    /** This build's version, as the phone reports it, or null when it does not. */
    private fun versionName(): String? = try {
        context.packageManager.getPackageInfo(context.packageName, 0).versionName
    } catch (e: Exception) {
        null
    }

    /** A column whose children are [gapDp] apart, as the design's flex gap. */
    private fun column(gapDp: Float): LinearLayout = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        showDividers = LinearLayout.SHOW_DIVIDER_MIDDLE
        dividerDrawable = GradientDrawable().apply {
            val size = dp(gapDp)
            setSize(size, size)
        }
    }

    /** The design's "Gap": room between sections, on top of the column's own gaps. */
    private fun space(heightDp: Float): View = View(context).apply {
        layoutParams = LinearLayout.LayoutParams(MATCH, dp(heightDp))
    }

    private fun dp(value: Float): Int = Ink.dp(context, value)
}
