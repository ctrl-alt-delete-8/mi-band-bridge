package dev.bandbridge.relay

import android.content.Context
import android.graphics.drawable.GradientDrawable
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import dev.bandbridge.relay.ui.Ink
import dev.bandbridge.relay.ui.MenuItem
import dev.bandbridge.relay.ui.Navigator
import dev.bandbridge.relay.ui.Page
import dev.bandbridge.relay.ui.Screen
import dev.bandbridge.relay.ui.Widgets

/**
 * The widget sheet: every widget of the kit once, in place of Main under
 * fixture `kit`, to be shot beside the design's components (Task 2, step 7).
 *
 * Each sample's content description names the component it draws, such as
 * `kit:ydfVT`, so a UI dump gives its bounds and a script can crop it for the
 * comparison. The small gray captions are for the person comparing. Neither
 * ships: this file is in the debug build only.
 */
internal class KitPage(private val context: Context, private val navigator: Navigator) : Page {
    override val title = "Mi Band Bridge"
    override val status = "Listening"

    private val body: View = column(16f).apply {
        setPadding(0, dp(16f), 0, dp(32f))
        // The app bar sample spans the gutters, and the navigator's gutter is
        // this view's padding, which would otherwise cut it off.
        clipToPadding = false

        caption("oKbsr uLhie eFBRk gXnpc ydfVT")
        val disabled = Widgets.primaryButton(context, "Save") {}.apply { enabled = false }
        addView(row(8f).apply {
            add(Widgets.button(context, "Button") {}, "oKbsr")
            add(Widgets.primaryButton(context, "Save") {}.view, "uLhie")
            add(disabled.view, "eFBRk")
            add(Widgets.iconButton(context, R.drawable.ic_x, "Remove rule") { openDialog() }, "gXnpc")
            add(Widgets.signalChip(context, "bb01"), "ydfVT")
        })

        caption("L2qEA6")
        addView(
            Widgets.appBar(context, "Mi Band Bridge", "Listening") {}.apply { contentDescription = "kit:L2qEA6" },
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(56f)).apply {
                // The component spans the phone's width, gutters included.
                leftMargin = -dp(16f)
                rightMargin = -dp(16f)
            },
        )

        caption("ALhwn")
        addView(Widgets.eventRow(context, "bb01", "Triggered Page down", "09:41:08").tagged("ALhwn"))

        caption("PKQF7")
        addView(
            Widgets.ruleRow(
                context, "bb01", "Page down", "3f2a9c1e-7b4d-4e0a-9c61-2d8e5f0a1b3c",
                onEdit = {}, onRemove = { openDialog() },
            ).tagged("PKQF7"),
        )

        caption("X9wJ4q")
        val name = Widgets.field(context, "Name", "For example, Page down", mono = false)
        name.input.setText("Page down")
        addView(name.view.tagged("X9wJ4q"))

        caption("field, mono, with help")
        addView(
            Widgets.field(
                context, "Key map id", "Paste the id from Key Mapper", mono = true,
                help = "Copy it from the key map’s Options tab in Key Mapper.",
            ).view.tagged("field-help"),
        )

        caption("field beside a button, frames 05 and 07")
        val signal = Widgets.field(
            context, "Signal", "—", mono = true,
            beside = Widgets.button(context, "Learn from the band", R.drawable.ic_radio) {},
        )
        signal.input.setText("bb01")
        addView(signal.view.tagged("signal-05"))
        addView(
            Widgets.field(
                context, "Signal", "—", mono = true,
                beside = Widgets.button(context, "Try again", R.drawable.ic_radio) {},
            ).view.tagged("signal-07"),
        )

        caption("columnHeader, sectionHead")
        addView(Widgets.columnHeader(context, "Edit rule") {}.tagged("columnHeader"))
        addView(Widgets.sectionHead(context, "REQUIRED APPS").tagged("sectionHead"))

        caption("listHeading with ⋮, quietRow, frame 02; ⋮ opens the menu, frame 16")
        lateinit var dots: View
        dots = Widgets.iconButton(context, R.drawable.ic_ellipsis_vertical, "Rules menu") { openMenu(dots) }
        addView(
            Widgets.listHeading(
                context, "Rules", "3", Widgets.button(context, "Add a rule", R.drawable.ic_plus) {}, menu = dots,
            ).tagged("listHeading"),
        )
        addView(
            Widgets.quietRow(context, R.drawable.ic_settings, "Setup") {}.tagged("quietRow"),
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                // The row spans the phone's width, as the page's footer does.
                leftMargin = -dp(16f)
                rightMargin = -dp(16f)
            },
        )

        caption("notice")
        addView(
            Widgets.notice(
                context,
                Widgets.text(context, "No press arrived in 10 seconds.", 15f, 600),
                Widgets.text(context, "Open setup →", 15f, 600),
            ).tagged("notice"),
        )

        caption("notice, the result line of frame 17")
        addView(
            Widgets.notice(
                context,
                Widgets.text(
                    context, Words.fileOnItsOwnLine(Fixtures.IMPORTED_LINE, Fixtures.IMPORTED_FILE), 14f, lineHeightSp = 20f,
                ),
                insetDp = 10f,
            ).tagged("notice-result"),
        )

        caption("countdown at 7 s")
        addView(Widgets.countdown(context).apply { show(7) }.view.tagged("countdown"))

        caption("listeningBox at 7 s, frame 06")
        addView(
            Widgets.listeningBox(
                context,
                title = "Waiting for a press…",
                text = "Press the button in the band app that you want to use.",
                countdown = Widgets.countdown(context).apply { show(7) },
            ) {}.tagged("listeningBox"),
        )

        caption("note, frame 05")
        addView(Widgets.note(context, "One signal can be in more than one rule. Every matching key map is sent.").tagged("note"))

        caption("resultMark, statusMark")
        addView(row(16f).apply {
            add(Widgets.resultMark(context, R.drawable.ic_radio, filled = false), "mark-radio")
            add(Widgets.resultMark(context, R.drawable.ic_check, filled = false), "mark-check")
            add(Widgets.resultMark(context, R.drawable.ic_arrow_left_right, filled = false), "mark-arrows")
            add(Widgets.resultMark(context, R.drawable.ic_x, filled = true), "mark-x")
            add(Widgets.statusMark(context, R.drawable.ic_check, filled = false), "status-check")
            add(Widgets.statusMark(context, R.drawable.ic_x, filled = true), "status-x")
        })

        caption("checklist, frames 14 and 15")
        addView(
            Widgets.checklist(
                context,
                "The band is connected (step 1).",
                "Both values match exactly (step 4).",
            ).tagged("checklist"),
        )

        caption("statusRow, downloadButtons, frames 01 and 08")
        addView(
            Widgets.statusRow(context, ok = true, "Gadgetbridge is installed", "Last band press arrived at 09:41:08.")
                .view.tagged("status-installed"),
        )
        addView(
            Widgets.statusRow(
                context, ok = false, "Gadgetbridge is not installed", "It sends band presses to this app.",
                Widgets.downloadButtons(context, "F-Droid" to {}, "Codeberg" to {}).tagged("downloadButtons"),
                Widgets.text(
                    context, "On Codeberg, download the file that starts with “gadgetbridge”, not “banglejs”.",
                    13f, color = Ink.DIM, lineHeightSp = 19f,
                ),
            ).view.tagged("status-missing"),
        )

        caption("step, copyField, frame 08, the second copied")
        addView(Widgets.step(context, 1, "Connect the band in Gadgetbridge.").tagged("step"))
        addView(
            Widgets.step(
                context, 4, "Copy these two values into the matching fields. Do not type them: one extra space stops all events.",
                Widgets.copyField(context, "BLE Characteristics", "00000017-0000-3512-2118-0009af100700").view.tagged("copyField"),
                Widgets.copyField(context, "BLE API package", "dev.bandbridge.relay").apply { copied = true }.view.tagged("copyField-copied"),
            ).tagged("step-copy"),
        )
        addView(
            Widgets.step(
                context, 7, "Check that band presses arrive.",
                Widgets.primaryButton(context, "Start check") {}.view,
                rule = false,
            ).tagged("step-last"),
        )

        caption("button with an icon")
        addView(
            Widgets.button(context, "Add a rule", R.drawable.ic_plus) {}.tagged("button-plus"),
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT),
        )
        addView(Widgets.button(context, "Learn from the band", R.drawable.ic_radio) {}.tagged("button-learn"))

        caption("icons")
        addView(row(12f).apply {
            val icons = listOf(
                R.drawable.ic_arrow_left, R.drawable.ic_arrow_left_right, R.drawable.ic_check,
                R.drawable.ic_chevron_right, R.drawable.ic_copy, R.drawable.ic_download,
                R.drawable.ic_ellipsis_vertical, R.drawable.ic_external_link, R.drawable.ic_info,
                R.drawable.ic_plus, R.drawable.ic_radio, R.drawable.ic_settings, R.drawable.ic_upload,
                R.drawable.ic_x,
            )
            for (icon in icons) {
                addView(Widgets.icon(context, icon, 22f))
            }
        })
        addView(row(12f).apply {
            addView(Widgets.icon(context, R.drawable.ic_settings, 18f, Ink.DIM))
            addView(Widgets.icon(context, R.drawable.ic_chevron_right, 18f, Ink.DIM))
            addView(Widgets.icon(context, R.drawable.ic_info, 18f, Ink.DIM))
        })

        // Review Focus 4: what fixture `long` holds must wrap inside a 412 dp
        // phone, and nothing may run off the edge or under ×.
        caption("long values")
        addView(
            Widgets.ruleRow(
                context, "bb04", "Scroll down in the reader app, then wait for the page",
                "5b9e1f7a-2c84-4d36-b0e5-7a1c9f3d2e68", onEdit = {}, onRemove = {},
            ).tagged("long-rule"),
        )
        addView(
            Widgets.eventRow(
                context, "0303", "Not a band app signal", "09:41:21",
                detail = Widgets.text(
                    context,
                    "030300520031000000" + "1B00" + (0 until 49).joinToString("") { it.toString(16).padStart(2, '0').uppercase() },
                    13f, color = Ink.DIM, mono = true,
                ),
            ).tagged("long-event"),
        )
        addView(row(8f).apply {
            add(Widgets.signalChip(context, "bb01020304050607"), "chip-longest-signal")
            add(Widgets.signalChip(context, "a signal kept as typed"), "chip-typed")
        })
    }

    private val footer: View = Widgets.footer(
        context,
        Widgets.text(context, "Add a signal, a name, and a key map id to save.", 13f, color = Ink.DIM),
        Widgets.primaryButton(context, "Save") {}.apply { enabled = false }.view,
        gapDp = 8f,
    ).tagged("footer")

    override fun body(): View = body

    override fun footer(): View = footer

    /** Frame 16's menu under [anchor], as ⋮ in the Rules header opens it. Its items do nothing here. */
    private fun openMenu(anchor: View) {
        Widgets.menu(
            navigator.host,
            anchor,
            listOf(
                MenuItem("Export rules", R.drawable.ic_download) {},
                MenuItem("Import rules", R.drawable.ic_upload) {},
            ),
        )
    }

    /** Frame 03's dialog, as × on the rule row opens it. */
    private fun openDialog() {
        Widgets.confirmDialog(
            navigator.host,
            title = "Remove this rule?",
            chip = "bb01",
            name = "Next slide",
            text = "bb01 will no longer trigger Next slide. You cannot undo this.",
            confirm = "Remove",
        ) { navigator.show(Screen.Setup(asRoot = false)) }
    }

    private fun LinearLayout.caption(text: String) {
        addView(Widgets.text(context, text, 12f, color = Ink.DIM, mono = true))
    }

    private fun LinearLayout.add(view: View, id: String) {
        addView(view.tagged(id))
    }

    private fun View.tagged(id: String): View = apply { contentDescription = "kit:$id" }

    private fun column(gapDp: Float): LinearLayout = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        showDividers = LinearLayout.SHOW_DIVIDER_MIDDLE
        dividerDrawable = gap(gapDp)
    }

    private fun row(gapDp: Float): LinearLayout = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        isBaselineAligned = false
        showDividers = LinearLayout.SHOW_DIVIDER_MIDDLE
        dividerDrawable = gap(gapDp)
    }

    private fun gap(gapDp: Float) = GradientDrawable().apply {
        setSize(dp(gapDp), dp(gapDp))
    }

    private fun dp(value: Float): Int = Ink.dp(context, value)
}
