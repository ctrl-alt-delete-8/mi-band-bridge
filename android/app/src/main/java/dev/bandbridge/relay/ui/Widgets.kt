package dev.bandbridge.relay.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.res.ColorStateList
import android.graphics.Canvas
import android.graphics.ColorFilter
import android.graphics.Paint
import android.graphics.PixelFormat
import android.graphics.Rect
import android.graphics.drawable.ColorDrawable
import android.graphics.drawable.Drawable
import android.graphics.drawable.GradientDrawable
import android.text.Editable
import android.text.InputFilter
import android.text.InputType
import android.text.Layout
import android.text.SpannableString
import android.text.Spanned
import android.text.TextUtils
import android.text.TextWatcher
import android.text.style.LineHeightSpan
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.ViewTreeObserver
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import androidx.core.view.isVisible
import dev.bandbridge.relay.LISTEN_SECONDS
import dev.bandbridge.relay.R
import kotlin.math.roundToInt

private const val MATCH = ViewGroup.LayoutParams.MATCH_PARENT
private const val WRAP = ViewGroup.LayoutParams.WRAP_CONTENT

/** Inter's line box as CSS calls it normal: (ascender 1984 + descender 494) / 2048 units per em. */
private const val INTER_LINE = 2478f / 2048f

/** JetBrains Mono's: (ascender 1020 + descender 300) / 1000 units per em. */
private const val MONO_LINE = 1.32f

/**
 * Marks the parts of a countdown, and what floats over the pages in the
 * navigator's host, the dialog's scrim or an open menu, so they can be found
 * again.
 */
private val SEGMENTS = Any()
private val SECONDS_LEFT = Any()
private val FLOATING = Any()

/**
 * The e-ink kit: every widget the four pages are built from, measured from
 * the design's components and frames.
 *
 * Views are plain and built in code, so nothing brings a ripple, a shadow, a
 * pressed state or an animation to undo. A tap changes what the screen says,
 * and that is its only feedback: an e-ink panel redraws slowly, and a flash
 * would cost it a full refresh. Sizes are the design's, at one CSS pixel to
 * one dp and one font pixel to one sp, and every tappable view is at least
 * 48 dp tall.
 *
 * The navigator sets the left and right padding of every view a page gives
 * it, so the gutter follows the screen. A widget's own padding, as built
 * here, is the phone's.
 */
object Widgets {
    /**
     * Words, in Inter, or in JetBrains Mono when [mono].
     *
     * Each line is exactly [lineHeightSp] tall when it is given, as the
     * design sets for running text. Otherwise it is the font's own line,
     * rounded to whole pixels as the design lays text out. Lines break where
     * CSS would break them, never with a hyphen.
     *
     * With [live], the words are the title of a state that takes the place
     * of another on the same page, such as a result of the check. They are a
     * polite live region, so a screen reader reads them after what it is
     * saying, once [reveal] shows their state.
     */
    fun text(
        context: Context,
        text: CharSequence,
        sizeSp: Float,
        weight: Int = 400,
        color: Int = Ink.BLACK,
        mono: Boolean = false,
        lineHeightSp: Float? = null,
        live: Boolean = false,
    ): TextView = inkText(context, text, sizeSp, weight, color, mono, lineHeightSp, live)

    /**
     * Shows [part] when [shown], else hides it. A part is one state of a
     * page that changes in place: a result of the check, or Learn's box or
     * notice.
     *
     * A person who cannot see the screen must still hear that the state
     * changed, so each part's title is [live]. But Android reads a live
     * region only when its words change, not when it appears, and from
     * Android 14 only when the new words differ from the old. So a live
     * title holds its words back while its part is hidden, and gets them
     * again as the part appears: a real change, which Android reads out. A
     * part already shown, or already hidden, is left alone, so laying a page
     * out again reads nothing.
     */
    fun reveal(part: View, shown: Boolean) {
        if (part.isVisible == shown) {
            return
        }
        part.isVisible = shown
        forEachLiveTitle(part) { title -> if (shown) title.sayWords() else title.holdWords() }
    }

    /**
     * Gives [view], made by [text], new [words], in step with [reveal]. While
     * the view's part is hidden, the words wait, and [reveal] says them as
     * the part appears. On a part already shown they change at once, and a
     * screen reader reads the change when the view was made live. The
     * result line under the Rules header works this way, since its words
     * differ each time it appears.
     */
    fun setWords(view: TextView, words: CharSequence) {
        if (view is InkText) {
            view.setWords(words)
        } else {
            view.text = words
        }
    }

    /**
     * An outlined button, 48 dp tall (component oKbsr), with an optional 18
     * dp icon before its label.
     *
     * The icon and the label stay together in the middle however wide the
     * button is laid out, as "Learn from the band" does in frame 05.
     */
    fun button(context: Context, label: String, icon: Int? = null, onClick: () -> Unit): TextView =
        inkText(context, label, 15f, 600).apply {
            gravity = Gravity.CENTER
            maxLines = 1
            ellipsize = TextUtils.TruncateAt.END
            minHeight = dp(context, 48f)
            background = box(context, Ink.WHITE, Ink.BLACK, 1.5f)
            setPadding(dp(context, 16f), 0, dp(context, 16f), 0)
            if (icon != null) {
                showIcon(icon, dp(context, 18f), dp(context, 8f))
            }
            tappable(onClick)
        }

    /** The filled black button, 52 dp tall (uLhie), which looks like eFBRk while it is disabled. */
    fun primaryButton(context: Context, label: String, onClick: () -> Unit): PrimaryButton {
        val view = inkText(context, label, 16f, 600).apply {
            gravity = Gravity.CENTER
            maxLines = 1
            ellipsize = TextUtils.TruncateAt.END
            minHeight = dp(context, 52f)
            setPadding(dp(context, 20f), 0, dp(context, 20f), 0)
        }
        val button = PrimaryButton(view)
        view.tappable { if (button.enabled) onClick() }
        return button
    }

    /** A 22 dp icon in a 48 by 48 dp target, with no outline (gXnpc): the back arrow, or × on a rule. */
    fun iconButton(context: Context, icon: Int, description: String, onClick: () -> Unit): ImageView =
        Widgets.icon(context, icon, 48f).apply {
            val inset = dp(context, (48f - 22f) / 2)
            setPadding(inset, inset, inset, inset)
            contentDescription = description
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_YES
            tappable(onClick)
        }

    /**
     * One Lucide icon, [sizeDp] square, in [color]. It says nothing to a
     * screen reader, because the words beside it do.
     */
    fun icon(context: Context, icon: Int, sizeDp: Float, color: Int = Ink.BLACK): ImageView =
        ImageView(context).apply {
            setImageResource(icon)
            imageTintList = ColorStateList.valueOf(color)
            scaleType = ImageView.ScaleType.FIT_CENTER
            val size = dp(context, sizeDp)
            layoutParams = ViewGroup.MarginLayoutParams(size, size)
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        }

    /**
     * A signal, or an event's label, in a 56 dp outlined chip (ydfVT).
     *
     * A longer label widens the chip, up to the longest signal the rule
     * allows. Text past that, which only a 1.0.0 signal kept as typed can
     * have, ends in an ellipsis rather than pushing the row off the screen.
     */
    fun signalChip(context: Context, text: String): TextView =
        inkText(context, text, 14f, 500, mono = true).apply {
            gravity = Gravity.CENTER
            maxLines = 1
            ellipsize = TextUtils.TruncateAt.END
            minWidth = dp(context, 56f)
            maxWidth = dp(context, 144f)
            setPadding(dp(context, 2f), dp(context, 3f), dp(context, 2f), dp(context, 3f))
            background = box(context, Ink.WHITE, Ink.BLACK, 1f)
        }

    /**
     * The app bar (L2qEA6): 56 dp, a black rule under it, the title, and
     * the status on the right. With [onBack], the arrow's 48 dp target comes
     * first and reaches 12 dp into the gutter, so on a phone it starts 4 dp
     * from the screen's edge and the title 56 dp, as frame 05 places them.
     */
    fun appBar(context: Context, title: String, status: String?, onBack: (() -> Unit)?): View =
        LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            isBaselineAligned = false
            gravity = Gravity.CENTER_VERTICAL
            background = Edges(Ink.BLACK, dp(context, 1f), Edges.BOTTOM)
            layoutParams = ViewGroup.MarginLayoutParams(MATCH, dp(context, 56f))
            setPadding(dp(context, 16f), 0, dp(context, 16f), 0)
            if (onBack != null) {
                addView(
                    iconButton(context, R.drawable.ic_arrow_left, "Back", onBack),
                    LinearLayout.LayoutParams(dp(context, 48f), dp(context, 48f)).apply {
                        leftMargin = -dp(context, 12f)
                        rightMargin = dp(context, 4f)
                    },
                )
            }
            addView(heading(context, title), LinearLayout.LayoutParams(0, WRAP, 1f))
            if (status != null) {
                addView(
                    inkText(context, status, 14f, color = Ink.DIM).apply { maxLines = 1 },
                    LinearLayout.LayoutParams(WRAP, WRAP).apply { leftMargin = dp(context, 16f) },
                )
            }
        }

    /**
     * The header of a page in a tablet's right column, 64 dp: the arrow and
     * the title (frame 10, "Editor header"). It has no rule, because the
     * column's own divider sets it apart.
     */
    fun columnHeader(context: Context, title: String, onBack: () -> Unit): View =
        LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            isBaselineAligned = false
            gravity = Gravity.CENTER_VERTICAL
            layoutParams = ViewGroup.MarginLayoutParams(MATCH, dp(context, 64f))
            addView(
                iconButton(context, R.drawable.ic_arrow_left, "Back", onBack),
                LinearLayout.LayoutParams(dp(context, 48f), dp(context, 48f)),
            )
            addView(
                heading(context, title),
                LinearLayout.LayoutParams(0, WRAP, 1f).apply { leftMargin = dp(context, 4f) },
            )
        }

    /**
     * The head of a list on the main page, 64 dp (frame 02's "Events
     * heading" and "Rules heading"): the [title], its [count] in gray when
     * there is one, and [action], such as Clear, at the far end. [menu], the
     * list's ⋮ button, comes last, 4 dp after [action], as frame 16's "Rules
     * actions" set them.
     *
     * The count sits on the title's line with the bottoms of their line
     * boxes aligned, as the design aligns them, so the smaller figure sits
     * low beside the title rather than centered on it.
     */
    fun listHeading(
        context: Context,
        title: String,
        count: String? = null,
        action: View? = null,
        menu: View? = null,
    ): View =
        LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            isBaselineAligned = false
            gravity = Gravity.CENTER_VERTICAL
            minimumHeight = dp(context, 64f)
            val words = LinearLayout(context).apply {
                orientation = LinearLayout.HORIZONTAL
                isBaselineAligned = false
                gravity = Gravity.BOTTOM
                addView(heading(context, title), LinearLayout.LayoutParams(WRAP, WRAP))
                if (count != null) {
                    addView(
                        inkText(context, count, 15f, color = Ink.DIM).apply { maxLines = 1 },
                        LinearLayout.LayoutParams(WRAP, WRAP).apply { leftMargin = dp(context, 8f) },
                    )
                }
            }
            addView(words, LinearLayout.LayoutParams(0, WRAP, 1f))
            if (action != null) {
                addView(action, LinearLayout.LayoutParams(WRAP, WRAP).apply { leftMargin = dp(context, 16f) })
            }
            if (menu != null) {
                addView(
                    menu,
                    LinearLayout.LayoutParams(dp(context, 48f), dp(context, 48f)).apply {
                        leftMargin = dp(context, if (action != null) 4f else 16f)
                    },
                )
            }
        }

    /**
     * A quiet row that leads to another page, 48 dp under a hairline
     * (frame 02's "Setup row · quiet"): an 18 dp icon and [label] in gray,
     * and a chevron at the far end. The whole row is the target.
     *
     * It is quiet on purpose, gray and with no status: the gate handles a
     * problem, not a banner here (the note, section 2).
     */
    fun quietRow(context: Context, icon: Int, label: String, onClick: () -> Unit): View =
        LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            isBaselineAligned = false
            gravity = Gravity.CENTER_VERTICAL
            background = Edges(Ink.HAIRLINE, dp(context, 1f), Edges.TOP)
            minimumHeight = dp(context, 48f)
            setPadding(dp(context, 16f), 0, dp(context, 16f), 0)
            addView(Widgets.icon(context, icon, 18f, Ink.DIM))
            addView(
                inkText(context, label, 15f, color = Ink.DIM).apply {
                    maxLines = 1
                    ellipsize = TextUtils.TruncateAt.END
                },
                LinearLayout.LayoutParams(0, WRAP, 1f).apply {
                    leftMargin = dp(context, 8f)
                    rightMargin = dp(context, 8f)
                },
            )
            addView(Widgets.icon(context, R.drawable.ic_chevron_right, 18f, Ink.DIM))
            tappable(onClick)
        }

    /**
     * A small gray heading in capitals, such as "REQUIRED APPS", with 1 dp
     * between its letters (frames 01 and 08). The caller writes the capitals,
     * as the design writes them.
     */
    fun sectionHead(context: Context, text: String): TextView =
        inkText(context, text, 12f, 600, Ink.DIM).apply { letterSpacing = 1f / 12f }

    /**
     * A required app's row on Setup, over a hairline (frames 01 and 08,
     * "Status"): its [statusMark], [title] and the gray [text] under it, and
     * [below] under those, such as the download buttons of a missing app.
     *
     * [ok] draws the outlined check of an installed app. Otherwise the mark
     * is a white × on black, the one place Setup says something is wrong,
     * with a filled mark rather than a color. The gray line can change in
     * place through [StatusRow.text], as the last band press line does.
     */
    fun statusRow(context: Context, ok: Boolean, title: String, text: String, vararg below: View): StatusRow {
        val line = Widgets.text(context, text, 13f, color = Ink.DIM, lineHeightSp = 19f)
        val view = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            isBaselineAligned = false
            gravity = Gravity.TOP
            background = Edges(Ink.HAIRLINE, dp(context, 1f), Edges.BOTTOM)
            setPadding(0, dp(context, 12f), 0, dp(context, 12f))
            val mark = if (ok) {
                statusMark(context, R.drawable.ic_check, filled = false)
            } else {
                statusMark(context, R.drawable.ic_x, filled = true)
            }
            addView(mark)
            val lines = column(context, 2f).apply {
                addView(inkText(context, title, 15f, 600), LinearLayout.LayoutParams(MATCH, WRAP))
                addView(line, LinearLayout.LayoutParams(MATCH, WRAP))
            }
            addView(
                column(context, 10f).apply {
                    addView(lines, LinearLayout.LayoutParams(MATCH, WRAP))
                    for (view in below) {
                        addView(view, LinearLayout.LayoutParams(MATCH, WRAP))
                    }
                },
                LinearLayout.LayoutParams(0, WRAP, 1f).apply { leftMargin = dp(context, 12f) },
            )
        }
        return StatusRow(view, line)
    }

    /**
     * One numbered step of Setup (frame 08, "Step"): [number] in a 24 dp
     * circle, [text] beside it, and [below] under the text, such as the
     * values to copy or Start check.
     *
     * A hairline closes each step, except with [rule] false: the last step
     * of a section, which the section's own space sets apart.
     */
    fun step(context: Context, number: Int, text: String, vararg below: View, rule: Boolean = true): View =
        LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            isBaselineAligned = false
            gravity = Gravity.TOP
            background = if (rule) Edges(Ink.HAIRLINE, dp(context, 1f), Edges.BOTTOM) else null
            setPadding(0, dp(context, 12f), 0, dp(context, 12f))
            addView(
                inkText(context, number.toString(), 13f, 600).apply {
                    gravity = Gravity.CENTER
                    background = GradientDrawable().apply {
                        shape = GradientDrawable.OVAL
                        setColor(Ink.WHITE)
                        setStroke(dp(context, 1.5f), Ink.BLACK)
                    }
                },
                LinearLayout.LayoutParams(dp(context, 24f), dp(context, 24f)),
            )
            addView(
                column(context, 10f).apply {
                    addView(Widgets.text(context, text, 15f, lineHeightSp = 22f), LinearLayout.LayoutParams(MATCH, WRAP))
                    for (view in below) {
                        addView(view, LinearLayout.LayoutParams(MATCH, WRAP))
                    }
                },
                LinearLayout.LayoutParams(0, WRAP, 1f).apply { leftMargin = dp(context, 12f) },
            )
        }

    /**
     * A missing app's download buttons (frame 01, "Download buttons"): an
     * outlined button with the external-link icon for each of [links], a
     * label and what it does, side by side and sharing the width, 8 dp
     * apart.
     */
    fun downloadButtons(context: Context, vararg links: Pair<String, () -> Unit>): View = row(context, 8f).apply {
        for ((label, onClick) in links) {
            addView(button(context, label, R.drawable.ic_external_link, onClick), LinearLayout.LayoutParams(0, WRAP, 1f))
        }
    }

    /**
     * A value to copy, read-only, with a copy button beside it (frames 01
     * and 08, step 4): the gray [label], [value] in JetBrains Mono inside a
     * hairline box, and the 48 dp button.
     *
     * The value is shown, not typed into a field, because it must reach
     * the other app exactly: one extra space stops every event. The button
     * puts the same [value] on the clipboard, so what is copied cannot
     * drift from what is shown, and its icon turns into a check mark (D13).
     * A value too long for a narrow screen wraps inside its box.
     */
    fun copyField(context: Context, label: String, value: String): CopyField {
        val box = inkText(context, value, 12f, mono = true).apply {
            gravity = Gravity.CENTER_VERTICAL or Gravity.START
            minHeight = dp(context, 48f)
            background = box(context, Ink.WHITE, Ink.HAIRLINE, 1f)
            val inset = dp(context, 8f)
            setPadding(inset, inset, inset, inset)
        }
        val button = Widgets.icon(context, R.drawable.ic_copy, 48f).apply {
            val inset = dp(context, (48f - 20f) / 2)
            setPadding(inset, inset, inset, inset)
            background = box(context, Ink.WHITE, Ink.BLACK, 1.5f)
            // The design names this layer Copy. A screen reader reads the
            // button on its own, and Setup has two, so each names its value.
            contentDescription = "Copy $label"
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_YES
        }
        val view = column(context, 6f).apply {
            addView(inkText(context, label, 13f, color = Ink.DIM), LinearLayout.LayoutParams(MATCH, WRAP))
            addView(
                row(context, 8f).apply {
                    addView(box, LinearLayout.LayoutParams(0, WRAP, 1f))
                    addView(button, LinearLayout.LayoutParams(dp(context, 48f), dp(context, 48f)))
                },
                LinearLayout.LayoutParams(MATCH, WRAP),
            )
        }
        val field = CopyField(view, button)
        button.tappable {
            context.getSystemService(ClipboardManager::class.java)?.setPrimaryClip(ClipData.newPlainText(label, value))
            field.copied = true
        }
        return field
    }

    /**
     * A labeled text field (X9wJ4q): the gray label, a 48 dp outlined input,
     * and the gray [help] under it when there is some.
     *
     * A [mono] field holds an id, so its value is JetBrains Mono 14 and no
     * keyboard suggestion can change it, while its hint stays Inter 16, as
     * frame 06 draws it. A long value wraps inside the box and the box grows,
     * so nothing runs off a narrow screen. Enter never adds a line: a rule's
     * name and id are one line each.
     *
     * With [beside], such as Learn from the band, the field is the short
     * signal input of frames 05 and 07: 96 dp wide, value and hint in
     * JetBrains Mono 16, and [beside] filling the rest of its row, 8 dp
     * away. That row is the input's parent, so a page can put something in
     * its place, as frame 06 does while it waits for a press.
     *
     * With [maxLength], the input takes no more characters than that. A
     * paste or a key past it is cut off there, by Android's own length
     * filter, which also cuts words set in the field from code.
     */
    fun field(
        context: Context,
        label: String,
        hint: String,
        mono: Boolean,
        help: String? = null,
        beside: View? = null,
        maxLength: Int? = null,
    ): Field {
        val input = EditText(context).apply {
            id = View.generateViewId()
            background = box(context, Ink.WHITE, Ink.BLACK, 1.5f)
            val inset = dp(context, 12f)
            setPadding(inset, inset, inset, inset)
            gravity = Gravity.CENTER_VERTICAL or Gravity.START
            includeFontPadding = false
            setTextColor(Ink.BLACK)
            setHintTextColor(Ink.HAIRLINE)
            highlightColor = Ink.HAIRLINE
            this.hint = hint
            inputType = InputType.TYPE_CLASS_TEXT or if (mono) {
                InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
            } else {
                InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
            }
            // The input type makes the field one scrolling line, exactly one
            // line tall. After it: wrap instead, with Enter still adding no
            // line break, and be at least the design's 48 dp. The minimum is
            // the view's own, which no line setting overrides.
            setHorizontallyScrolling(false)
            maxLines = Int.MAX_VALUE
            minimumHeight = dp(context, 48f)
            if (maxLength != null) {
                filters = arrayOf(InputFilter.LengthFilter(maxLength))
            }
        }
        val labelView = inkText(context, label, 14f, color = Ink.DIM).apply { labelFor = input.id }
        val helpView = help?.let { text(context, it, 13f, color = Ink.DIM, lineHeightSp = 18f) }
        val view = column(context, 6f).apply {
            addView(labelView, LinearLayout.LayoutParams(MATCH, WRAP))
            if (beside == null) {
                addView(input, LinearLayout.LayoutParams(MATCH, WRAP))
            } else {
                addView(
                    row(context, 8f).apply {
                        addView(input, LinearLayout.LayoutParams(dp(context, 96f), WRAP))
                        addView(beside, LinearLayout.LayoutParams(0, WRAP, 1f))
                    },
                    LinearLayout.LayoutParams(MATCH, WRAP),
                )
            }
            if (helpView != null) {
                addView(helpView, LinearLayout.LayoutParams(MATCH, WRAP))
            }
        }
        val short = beside != null
        styleInput(context, input, valueMono = mono, valueSp = if (mono && !short) 14f else 16f, hintMono = mono && short)
        return Field(view, input, helpView)
    }

    /**
     * A notice, set apart by a 3 dp black bar on its left (frames 07, 08
     * and 17): [children] one under another, 6 dp apart, their words 14 dp
     * in from the page's edge, as the design places them, and [insetDp]
     * above and below them: 12, or 10 for the result line under the Rules
     * header (frame 17).
     *
     * The design centers the bar on the page's edge, half of it in the
     * gutter. Here it is drawn inside the notice, 1.5 dp further in, since
     * any container between the notice and the page would cut off a bar
     * that reached out of it.
     *
     * With [onTap], the whole notice is the target of the link it ends with,
     * such as frame 07's "Open setup →". One line of text is far short of
     * 48 dp, and the notice is not.
     */
    fun notice(
        context: Context,
        vararg children: View,
        onTap: (() -> Unit)? = null,
        insetDp: Float = 12f,
    ): View = column(context, 6f).apply {
        background = Edges(Ink.BLACK, dp(context, 3f), Edges.LEFT)
        setPadding(dp(context, 14f), dp(context, insetDp), dp(context, 14f), dp(context, insetDp))
        for (child in children) {
            addView(child, LinearLayout.LayoutParams(MATCH, WRAP))
        }
        if (onTap != null) {
            tappable(onTap)
        }
    }

    /**
     * The box that takes the place of the signal row while Learn waits for a
     * press (frame 06, "Listening box"): the radio icon and [title], [text],
     * the [countdown], and Cancel, inside a 2 dp black outline.
     *
     * Cancel runs [onCancel]. The box is the one place in the app where a
     * band press is taken on purpose, so it says so in [text], and the
     * countdown shows how long it lasts.
     */
    fun listeningBox(context: Context, title: String, text: String, countdown: Countdown, onCancel: () -> Unit): View =
        column(context, 12f).apply {
            background = box(context, Ink.WHITE, Ink.BLACK, 2f)
            val inset = dp(context, 16f)
            setPadding(inset, inset, inset, inset)
            addView(
                row(context, 8f).apply {
                    gravity = Gravity.CENTER_VERTICAL
                    addView(Widgets.icon(context, R.drawable.ic_radio, 20f))
                    addView(inkText(context, title, 16f, 600, live = true), LinearLayout.LayoutParams(0, WRAP, 1f))
                },
                LinearLayout.LayoutParams(MATCH, WRAP),
            )
            addView(Widgets.text(context, text, 14f, lineHeightSp = 20f), LinearLayout.LayoutParams(MATCH, WRAP))
            addView(countdown.view, LinearLayout.LayoutParams(MATCH, WRAP))
            addView(button(context, "Cancel", onClick = onCancel), LinearLayout.LayoutParams(MATCH, WRAP))
        }

    /**
     * A quiet note at the foot of a page, under a hairline (frames 05 to 07,
     * "Note"): the info icon beside the first line of [text], both gray.
     */
    fun note(context: Context, text: String): View = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        isBaselineAligned = false
        gravity = Gravity.TOP
        background = Edges(Ink.HAIRLINE, dp(context, 1f), Edges.TOP)
        setPadding(0, dp(context, 14f), 0, 0)
        addView(Widgets.icon(context, R.drawable.ic_info, 18f, Ink.DIM))
        addView(
            Widgets.text(context, text, 13f, color = Ink.DIM, lineHeightSp = 19f),
            LinearLayout.LayoutParams(0, WRAP, 1f).apply { leftMargin = dp(context, 10f) },
        )
    }

    /**
     * The listening countdown (frames 06 and 11): one segment for each of the
     * [LISTEN_SECONDS] seconds, then "Watching for 10 seconds" and the
     * seconds left. It starts full, with nothing filled.
     */
    fun countdown(context: Context): Countdown {
        val segments = row(context, 4f).apply { tag = SEGMENTS }
        repeat(LISTEN_SECONDS) {
            segments.addView(
                View(context).apply { background = box(context, Ink.WHITE, Ink.BLACK, 1f) },
                LinearLayout.LayoutParams(0, dp(context, 10f), 1f),
            )
        }
        val words = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            isBaselineAligned = false
            addView(
                inkText(context, "Watching for $LISTEN_SECONDS seconds", 13f, color = Ink.DIM),
                LinearLayout.LayoutParams(0, WRAP, 1f),
            )
            addView(
                inkText(context, "", 13f, 600, mono = true).apply { tag = SECONDS_LEFT },
                LinearLayout.LayoutParams(WRAP, WRAP),
            )
        }
        val view = column(context, 6f).apply {
            addView(segments, LinearLayout.LayoutParams(MATCH, WRAP))
            addView(words, LinearLayout.LayoutParams(MATCH, WRAP))
        }
        return Countdown(view).also { it.show(LISTEN_SECONDS) }
    }

    /**
     * What to check, one item under another, 10 dp apart (frames 14 and 15,
     * "Checklist"): each of [items] in Inter 15 beside a 6 dp black square.
     *
     * The square sits on the middle of the item's first line however many
     * lines the item wraps to, as the design centers its bullet in a box one
     * line tall, and the words start 24 dp in: that 14 dp box and a 10 dp
     * gap. The square says nothing to a screen reader.
     */
    fun checklist(context: Context, vararg items: String): View = column(context, 10f).apply {
        val line = sp(context, 22f)
        val square = dp(context, 6f)
        for (item in items) {
            addView(
                LinearLayout(context).apply {
                    orientation = LinearLayout.HORIZONTAL
                    isBaselineAligned = false
                    gravity = Gravity.TOP
                    addView(
                        View(context).apply {
                            setBackgroundColor(Ink.BLACK)
                            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
                        },
                        LinearLayout.LayoutParams(square, square).apply {
                            topMargin = (line - square) / 2
                            rightMargin = dp(context, 14f) - square + dp(context, 10f)
                        },
                    )
                    addView(Widgets.text(context, item, 15f, lineHeightSp = 22f), LinearLayout.LayoutParams(0, WRAP, 1f))
                },
                LinearLayout.LayoutParams(MATCH, WRAP),
            )
        }
    }

    /**
     * The check's 56 dp result mark (frames 11 to 15): an outlined square
     * with a 28 dp icon, or a filled black one with a white icon when
     * something failed.
     */
    fun resultMark(context: Context, icon: Int, filled: Boolean): View =
        mark(context, icon, filled, sizeDp = 56f, iconDp = 28f, strokeDp = 2f)

    /**
     * Setup's 24 dp status mark (frames 01 and 08): the same square as
     * [resultMark] at the height of a title, with a 16 dp icon and a 1.5 dp
     * outline, filled black with a white icon when an app is missing.
     */
    fun statusMark(context: Context, icon: Int, filled: Boolean): View =
        mark(context, icon, filled, sizeDp = 24f, iconDp = 16f, strokeDp = 1.5f)

    /**
     * The footer pinned under a page's body, such as Save, with a black rule
     * above it: [children] one under another, full width.
     *
     * [gapDp] is 12 between two buttons (frames 11 to 15) and 8 under the
     * line that says what Save is missing (frames 06 and 07). A line of
     * text is centered, as those frames draw it. A child that is gone takes
     * its gap with it.
     */
    fun footer(context: Context, vararg children: View, gapDp: Float = 12f): View = FooterBar(context).apply {
        dividerDrawable = gap(context, gapDp)
        showDividers = LinearLayout.SHOW_DIVIDER_MIDDLE
        setPadding(dp(context, 16f), paddingTop, dp(context, 16f), paddingBottom)
        for (child in children) {
            if (child is TextView && !child.isClickable) {
                child.gravity = Gravity.CENTER_HORIZONTAL
            }
            addView(child, LinearLayout.LayoutParams(MATCH, WRAP))
        }
    }

    /**
     * The confirm dialog of frame 03, over everything in [host]: [title],
     * the rule it is about as a chip and a name, [text], Cancel, and
     * [confirm].
     *
     * It stays until the person answers. Cancel closes it, as System Back
     * does through the navigator. [confirm] closes it, then runs
     * [onConfirm]. A tap on the scrim does nothing, so a stray touch on an
     * e-ink screen never answers for the person. At most one dialog or menu
     * is open.
     */
    fun confirmDialog(
        host: FrameLayout,
        title: String,
        chip: String,
        name: String,
        text: String,
        confirm: String,
        onConfirm: () -> Unit,
    ) {
        val context = host.context
        closeFloating(host)
        val scrim = FrameLayout(context).apply {
            tag = FLOATING
            setBackgroundColor(Ink.SCRIM)
            isClickable = true
            val margin = dp(context, 24f)
            setPadding(margin, margin, margin, margin)
        }
        val summary = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            isBaselineAligned = false
            gravity = Gravity.CENTER_VERTICAL
            background = Edges(Ink.HAIRLINE, dp(context, 1f), Edges.TOP or Edges.BOTTOM)
            setPadding(0, dp(context, 12f), 0, dp(context, 12f))
            addView(signalChip(context, chip), LinearLayout.LayoutParams(WRAP, WRAP))
            addView(
                inkText(context, name, 16f, 600),
                LinearLayout.LayoutParams(0, WRAP, 1f).apply { leftMargin = dp(context, 10f) },
            )
        }
        val confirmButton = primaryButton(context, confirm) {
            closeFloating(host)
            onConfirm()
        }.view.apply { minHeight = dp(context, 48f) }
        val buttons = row(context, 12f).apply {
            addView(button(context, "Cancel") { closeFloating(host) }, LinearLayout.LayoutParams(0, WRAP, 1f))
            addView(confirmButton, LinearLayout.LayoutParams(0, WRAP, 1f))
        }
        val dialog = column(context, 16f).apply {
            background = box(context, Ink.WHITE, Ink.BLACK, 2f)
            isClickable = true
            val inset = dp(context, 24f)
            setPadding(inset, inset, inset, inset)
            addView(inkText(context, title, 20f, 600), LinearLayout.LayoutParams(MATCH, WRAP))
            addView(summary, LinearLayout.LayoutParams(MATCH, WRAP))
            addView(Widgets.text(context, text, 15f, lineHeightSp = 22f), LinearLayout.LayoutParams(MATCH, WRAP))
            addView(buttons, LinearLayout.LayoutParams(MATCH, WRAP))
        }
        // On a phone the dialog spans the screen less its margins. On a
        // tablet it stops at the right column's width.
        val capped = CappedWidth(context, dp(context, 560f)).apply {
            addView(dialog, FrameLayout.LayoutParams(MATCH, WRAP))
        }
        scrim.addView(capped, FrameLayout.LayoutParams(MATCH, WRAP, Gravity.CENTER))
        host.addView(scrim, FrameLayout.LayoutParams(MATCH, MATCH))
    }

    /**
     * A small menu under [anchor], over every page in [host] (frame 16,
     * "Rules menu · open"): [items] one under another, 220 dp wide, in a
     * 2 dp black outline, with a hairline between two items.
     *
     * It opens with its right edge on the anchor's right edge, 4 dp under
     * it, and keeps to the anchor if the page moves under it (see
     * [MenuLayer]). It has no scrim, yet the rest of the screen belongs to
     * it while it is open: a tap anywhere else, on the anchor too, closes it
     * and reaches nothing under it, so a stray second tap on an e-ink panel
     * cannot open the rule under the menu. System Back closes it through the
     * navigator. A tap on an item closes the menu, then runs the item. An
     * item that is not enabled is gray, as a disabled button is, and ignores
     * taps. At most one dialog or menu is open.
     */
    fun menu(host: FrameLayout, anchor: View, items: List<MenuItem>) {
        val context = host.context
        closeFloating(host)
        val stroke = dp(context, 2f)
        val box = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            background = box(context, Ink.WHITE, Ink.BLACK, 2f)
            // It takes a touch between its items, so that the touch does not
            // close it. It says nothing to a screen reader, whose items do.
            isClickable = true
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
            items.forEachIndexed { index, item ->
                if (index > 0) {
                    // The hairline meets the outline, which is drawn inside the box.
                    addView(
                        View(context).apply { setBackgroundColor(Ink.HAIRLINE) },
                        LinearLayout.LayoutParams(MATCH, dp(context, 1f)).apply {
                            leftMargin = stroke
                            rightMargin = stroke
                        },
                    )
                }
                addView(
                    menuItem(context, item) {
                        closeFloating(host)
                        item.onTap()
                    },
                    LinearLayout.LayoutParams(MATCH, WRAP),
                )
            }
        }
        val layer = MenuLayer(context, anchor, box, gapPx = dp(context, 4f)).apply {
            tag = FLOATING
            tappable { closeFloating(host) }
        }
        host.addView(layer, FrameLayout.LayoutParams(MATCH, MATCH))
    }

    /**
     * One event row (ALhwn): the chip, the outcome, and the time on the
     * right, over a hairline. [detail] goes under the outcome, such as the
     * raw bytes of other band traffic.
     *
     * With [onTap], the whole row is the target of the link its detail
     * shows, such as "Add a rule for bb03". The link's one line of 13 sp
     * text is far short of 48 dp, and the row is not.
     */
    fun eventRow(
        context: Context,
        label: String,
        outcome: String,
        time: String,
        detail: View? = null,
        onTap: (() -> Unit)? = null,
    ): View =
        LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            isBaselineAligned = false
            gravity = Gravity.TOP
            background = Edges(Ink.HAIRLINE, dp(context, 1f), Edges.BOTTOM)
            setPadding(0, dp(context, 10f), 0, dp(context, 10f))
            addView(signalChip(context, label), LinearLayout.LayoutParams(WRAP, WRAP))
            val body = column(context, 3f).apply {
                addView(text(context, outcome, 15f, lineHeightSp = 20f), LinearLayout.LayoutParams(MATCH, WRAP))
                if (detail != null) {
                    addView(detail, LinearLayout.LayoutParams(MATCH, WRAP))
                }
            }
            addView(
                body,
                LinearLayout.LayoutParams(0, WRAP, 1f).apply {
                    leftMargin = dp(context, 12f)
                    rightMargin = dp(context, 12f)
                },
            )
            addView(
                text(context, time, 13f, color = Ink.DIM, mono = true, lineHeightSp = 20f),
                LinearLayout.LayoutParams(WRAP, WRAP),
            )
            if (onTap != null) {
                tappable(onTap)
            }
        }

    /**
     * One rule row (PKQF7): the chip and the name, the key map id under
     * them, and × on the right, over a hairline.
     *
     * A tap anywhere but × runs [onEdit], and × runs [onRemove]. A long name
     * or id wraps before ×, and the name's first line stays centered on the
     * chip, where the design puts a name that fits.
     */
    fun ruleRow(
        context: Context,
        signal: String,
        name: String,
        keyMapUid: String,
        onEdit: () -> Unit,
        onRemove: () -> Unit,
    ): View = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        isBaselineAligned = false
        gravity = Gravity.CENTER_VERTICAL
        background = Edges(Ink.HAIRLINE, dp(context, 1f), Edges.BOTTOM)
        val chip = signalChip(context, signal)
        val nameView = inkText(context, name, 16f, 600).apply {
            // Center the first line on the chip: the chip is its line and 3 dp above and below.
            val chipHeight = sp(context, naturalLine(14f, mono = true)) + 2 * dp(context, 3f)
            setPadding(0, ((chipHeight - lineHeightPx) / 2).coerceAtLeast(0), 0, 0)
        }
        val head = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            isBaselineAligned = false
            gravity = Gravity.TOP
            addView(chip, LinearLayout.LayoutParams(WRAP, WRAP))
            addView(nameView, LinearLayout.LayoutParams(0, WRAP, 1f).apply { leftMargin = dp(context, 10f) })
        }
        val edit = column(context, 6f).apply {
            setPadding(0, dp(context, 12f), 0, dp(context, 12f))
            addView(head, LinearLayout.LayoutParams(MATCH, WRAP))
            addView(text(context, keyMapUid, 13f, color = Ink.DIM, mono = true), LinearLayout.LayoutParams(MATCH, WRAP))
            tappable(onEdit)
        }
        addView(edit, LinearLayout.LayoutParams(0, WRAP, 1f))
        addView(
            // The design's name for this layer. "Remove" alone is the
            // dialog's confirm button, and a screen reader would read both.
            iconButton(context, R.drawable.ic_x, "Remove rule", onRemove),
            LinearLayout.LayoutParams(dp(context, 48f), dp(context, 48f)).apply { leftMargin = dp(context, 8f) },
        )
    }

    /** Closes the dialog or the menu open in [host], if there is one, and says whether there was. */
    internal fun closeFloating(host: FrameLayout): Boolean {
        for (i in host.childCount - 1 downTo 0) {
            val child = host.getChildAt(i)
            if (child.tag === FLOATING) {
                host.removeViewAt(i)
                return true
            }
        }
        return false
    }

    /**
     * One item of [menu], 52 dp tall: its 20 dp icon and its label, 12 dp
     * apart, 16 dp in from each side. The whole row is the target.
     */
    private fun menuItem(context: Context, item: MenuItem, onTap: () -> Unit): View = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        isBaselineAligned = false
        gravity = Gravity.CENTER_VERTICAL
        minimumHeight = dp(context, 52f)
        setPadding(dp(context, 16f), 0, dp(context, 16f), 0)
        val color = if (item.enabled) Ink.BLACK else Ink.HAIRLINE
        addView(Widgets.icon(context, item.icon, 20f, color))
        addView(
            inkText(context, item.label, 15f, 500, color),
            LinearLayout.LayoutParams(0, WRAP, 1f).apply { leftMargin = dp(context, 12f) },
        )
        // A disabled row still takes its touch, and does nothing with it.
        isEnabled = item.enabled
        tappable(onTap)
    }

    /** An outlined square around a centered icon, or a filled one with the icon in white. */
    private fun mark(context: Context, icon: Int, filled: Boolean, sizeDp: Float, iconDp: Float, strokeDp: Float): View =
        Widgets.icon(context, icon, sizeDp, if (filled) Ink.WHITE else Ink.BLACK).apply {
            val inset = dp(context, (sizeDp - iconDp) / 2)
            setPadding(inset, inset, inset, inset)
            background = box(context, if (filled) Ink.BLACK else Ink.WHITE, Ink.BLACK, strokeDp)
        }

    /** A page or column title: Inter 20 semibold, on one line. */
    private fun heading(context: Context, title: String): TextView =
        inkText(context, title, 20f, 600).apply {
            maxLines = 1
            ellipsize = TextUtils.TruncateAt.END
        }

    private fun inkText(
        context: Context,
        text: CharSequence,
        sizeSp: Float,
        weight: Int = 400,
        color: Int = Ink.BLACK,
        mono: Boolean = false,
        lineHeightSp: Float? = null,
        live: Boolean = false,
    ): InkText = InkText(context).apply {
        typeface = if (mono) Ink.mono(context, weight) else Ink.inter(context, weight)
        setTextSize(TypedValue.COMPLEX_UNIT_SP, sizeSp)
        setTextColor(color)
        includeFontPadding = false
        breakStrategy = Layout.BREAK_STRATEGY_SIMPLE
        hyphenationFrequency = Layout.HYPHENATION_FREQUENCY_NONE
        lineHeightPx = sp(context, lineHeightSp ?: naturalLine(sizeSp, mono))
        if (live) {
            accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
        }
        this.text = text
    }

    /** Runs [action] on each live title in [view], [view] itself included, for [reveal]. */
    private fun forEachLiveTitle(view: View, action: (InkText) -> Unit) {
        if (view is InkText && view.accessibilityLiveRegion != View.ACCESSIBILITY_LIVE_REGION_NONE) {
            action(view)
        }
        if (view is ViewGroup) {
            for (i in 0 until view.childCount) {
                forEachLiveTitle(view.getChildAt(i), action)
            }
        }
    }

    /**
     * A field's hint and its value can differ in font and size: the key map
     * id's hint is Inter 16 and its value JetBrains Mono 14. So the look
     * follows whether the field is empty, and changes only when that does,
     * not on every key.
     */
    private fun styleInput(context: Context, input: EditText, valueMono: Boolean, valueSp: Float, hintMono: Boolean) {
        var showingValue: Boolean? = null
        fun look() {
            val value = input.text.isNotEmpty()
            if (value == showingValue) {
                return
            }
            showingValue = value
            val mono = if (value) valueMono else hintMono
            input.typeface = if (mono) Ink.mono(context, 400) else Ink.inter(context, 400)
            input.setTextSize(TypedValue.COMPLEX_UNIT_SP, if (value) valueSp else 16f)
        }
        look()
        input.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) = Unit
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) = Unit
            override fun afterTextChanged(s: Editable?) = look()
        })
    }

    private fun View.tappable(onClick: () -> Unit) {
        setOnClickListener { onClick() }
        // A focused view would get the platform's translucent highlight,
        // which is not one of the four colors.
        defaultFocusHighlightEnabled = false
    }
}

/**
 * The primary button of [Widgets.primaryButton]. Disabled, it has the look
 * of eFBRk, a gray outline and gray words, and ignores taps.
 */
class PrimaryButton(val view: TextView) {
    var enabled: Boolean = true
        set(value) {
            field = value
            look()
        }

    init {
        look()
    }

    private fun look() {
        view.isEnabled = enabled
        if (enabled) {
            view.background = ColorDrawable(Ink.BLACK)
            view.setTextColor(Ink.WHITE)
        } else {
            view.background = box(view.context, Ink.WHITE, Ink.HAIRLINE, 1.5f)
            view.setTextColor(Ink.HAIRLINE)
        }
    }
}

/**
 * A field of [Widgets.field]: the whole labeled [view], the [input] to read
 * and fill, and its [help] line when it has one, which a page may hide.
 */
class Field(val view: View, val input: EditText, val help: TextView? = null)

/**
 * A required app's row, from [Widgets.statusRow]: the whole [view], and the
 * gray line under its title, which a page can change in place.
 */
class StatusRow(val view: View, private val line: TextView) {
    /**
     * The gray line's words. Setup changes them as band presses arrive. The
     * words the line already has redraw nothing, so an e-ink panel redraws
     * one line for a press, and nothing for a refresh that finds no new one.
     */
    var text: String
        get() = line.text.toString()
        set(value) {
            if (value != line.text.toString()) {
                line.text = value
            }
        }
}

/**
 * A value to copy, from [Widgets.copyField]: the whole labeled [view], and
 * whether the value was [copied].
 *
 * A tap on the copy button sets [copied], and the button then shows a check
 * mark in place of the copy icon. The mark stays as long as the field: a page
 * builds its views once and keeps them while it is on the navigator's stack
 * (D13). There is no toast: the mark is the tap's only feedback, and it does
 * not go away by itself.
 */
class CopyField(val view: View, private val button: ImageView) {
    var copied: Boolean = false
        set(value) {
            if (value == field) {
                return
            }
            field = value
            button.setImageResource(if (value) R.drawable.ic_check else R.drawable.ic_copy)
        }
}

/**
 * One item of [Widgets.menu]: its [label], its Lucide [icon], whether it is
 * [enabled], and what a tap on it does. A disabled item is gray and ignores
 * taps, as Export rules is while there are no rules (B2).
 */
class MenuItem(val label: String, val icon: Int, val enabled: Boolean = true, val onTap: () -> Unit)

/**
 * An open menu of [Widgets.menu]: a clear layer over the whole of the
 * navigator's host, and in it the menu's [box], where [MenuSpot] puts it for
 * the [anchor], [gapPx] under it.
 *
 * The anchor is a view of the page, and the page can move under the menu: a
 * band press adds an event above the Rules header, or the screen turns. So
 * before each frame is drawn the layer looks where the anchor is now, and
 * when it has moved, lays the box out again before anything is drawn, so the
 * menu is never drawn in the old place first. The layer itself says nothing
 * to a screen reader: its touches only close the menu, which Back does too.
 */
internal class MenuLayer(
    context: Context,
    private val anchor: View,
    private val box: View,
    private val gapPx: Int,
) : FrameLayout(context) {
    private val anchorAt = IntArray(2)
    private val layerAt = IntArray(2)

    /** Where the anchor was when the box was last laid out. */
    private var placedFor: Rect? = null

    private val follow = ViewTreeObserver.OnPreDrawListener {
        val now = anchorBounds()
        if (now == null || now == placedFor) {
            true
        } else {
            requestLayout()
            false
        }
    }

    init {
        importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        addView(box, LayoutParams(Ink.dp(context, 220f), LayoutParams.WRAP_CONTENT))
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        viewTreeObserver.addOnPreDrawListener(follow)
    }

    override fun onDetachedFromWindow() {
        viewTreeObserver.removeOnPreDrawListener(follow)
        super.onDetachedFromWindow()
    }

    /** Lays the box out against the anchor. While the anchor is off the screen, the box stays where it is. */
    override fun onLayout(changed: Boolean, left: Int, top: Int, right: Int, bottom: Int) {
        val anchored = anchorBounds() ?: return
        placedFor = anchored
        val spot = MenuSpot.of(
            anchorRight = anchored.right,
            anchorTop = anchored.top,
            anchorBottom = anchored.bottom,
            width = box.measuredWidth,
            height = box.measuredHeight,
            gap = gapPx,
            layerHeight = height,
        )
        box.layout(spot.left, spot.top, spot.left + box.measuredWidth, spot.top + box.measuredHeight)
    }

    /** The anchor's edges in this layer, or null while either is off the screen. */
    private fun anchorBounds(): Rect? {
        if (!isAttachedToWindow || !anchor.isAttachedToWindow || !anchor.isShown) {
            return null
        }
        anchor.getLocationInWindow(anchorAt)
        getLocationInWindow(layerAt)
        val left = anchorAt[0] - layerAt[0]
        val top = anchorAt[1] - layerAt[1]
        return Rect(left, top, left + anchor.width, top + anchor.height)
    }
}

/**
 * Where an open menu's box goes in its layer: its [left] and [top] edges.
 * Pure, so the rule is tested without a device.
 */
internal data class MenuSpot(val left: Int, val top: Int) {
    companion object {
        /**
         * Under the anchor, [gap] below it, with its right edge on the
         * anchor's right edge, as frame 16 draws it. When the layer has no
         * room there, above the anchor, the same gap away, so the last item
         * is never cut off by the foot of a short window. With room on
         * neither side, as low as the whole menu fits. It never runs off the
         * left edge.
         */
        fun of(
            anchorRight: Int,
            anchorTop: Int,
            anchorBottom: Int,
            width: Int,
            height: Int,
            gap: Int,
            layerHeight: Int,
        ): MenuSpot {
            val below = anchorBottom + gap
            val above = anchorTop - gap - height
            val top = when {
                below + height <= layerHeight -> below
                above >= 0 -> above
                else -> (layerHeight - height).coerceAtLeast(0)
            }
            return MenuSpot((anchorRight - width).coerceAtLeast(0), top)
        }
    }
}

/** The countdown of [Widgets.countdown]. */
class Countdown(val view: View) {
    /**
     * Fills one segment for each second gone, [LISTEN_SECONDS] minus
     * [secondsLeft], and writes "7 s left". The page calls it once a
     * second, so the panel redraws once a second and no more.
     */
    fun show(secondsLeft: Int) {
        val left = secondsLeft.coerceIn(0, LISTEN_SECONDS)
        val segments = view.findViewWithTag<ViewGroup>(SEGMENTS)
        for (i in 0 until segments.childCount) {
            val segment = segments.getChildAt(i).background as GradientDrawable
            segment.setColor(if (i < LISTEN_SECONDS - left) Ink.BLACK else Ink.WHITE)
        }
        view.findViewWithTag<TextView>(SECONDS_LEFT).text = "$left s left"
    }
}

/**
 * A TextView that lays its lines out as the design does, and can draw an
 * icon before a centered label.
 *
 * With [lineHeightPx] set, each line is exactly that tall, with the spare
 * room split above and below the text, as CSS splits a line height. Android
 * would otherwise make each line as tall as the font's metrics rounded up, a
 * little taller than the design, and the difference adds up down a page.
 * The height is kept when the words change, so a page can call setText
 * freely.
 */
internal class InkText(context: Context) : TextView(context) {
    private var exactLine: ExactLine? = null
    private var icon: Drawable? = null
    private var iconPx = 0
    private var iconGapPx = 0

    /** A live title's words, held back while its part of the page is hidden (see [Widgets.reveal]). */
    private var heldWords: CharSequence? = null

    /** Holds the words back, for a part that is hidden. */
    fun holdWords() {
        if (heldWords == null) {
            heldWords = text
            text = ""
        }
    }

    /** Gives the held words back, for a part that appears: a change of words, which a screen reader reads. */
    fun sayWords() {
        val words = heldWords ?: return
        heldWords = null
        text = words
    }

    /** New words: held back with the part while it is hidden, or shown at once (see [Widgets.setWords]). */
    fun setWords(words: CharSequence) {
        if (heldWords != null) {
            heldWords = words
        } else {
            text = words
        }
    }

    var lineHeightPx: Int = 0
        set(value) {
            field = value
            exactLine = if (value > 0) ExactLine(value) else null
            text = text
        }

    /**
     * Draws [iconId] before the label, [sizePx] square and [gapPx] from it.
     *
     * The left padding grows by the icon and the gap, so the label centers
     * in what is left, and the icon and the label together center in the
     * whole button.
     */
    fun showIcon(iconId: Int, sizePx: Int, gapPx: Int) {
        val drawable = context.getDrawable(iconId)?.mutate() ?: return
        drawable.setTint(currentTextColor)
        val added = sizePx + gapPx - (iconPx + iconGapPx)
        icon = drawable
        iconPx = sizePx
        iconGapPx = gapPx
        setPadding(paddingLeft + added, paddingTop, paddingRight, paddingBottom)
    }

    /** Wraps new words in the line height, whatever their source. TextView's constructor calls this before [exactLine] exists. */
    override fun setText(text: CharSequence?, type: BufferType?) {
        val line = exactLine
        if (line == null || text == null) {
            super.setText(text, type)
            return
        }
        val spanned = SpannableString(text)
        for (old in spanned.getSpans(0, spanned.length, ExactLine::class.java)) {
            spanned.removeSpan(old)
        }
        spanned.setSpan(line, 0, spanned.length, Spanned.SPAN_INCLUSIVE_INCLUSIVE)
        super.setText(spanned, type)
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val drawable = icon ?: return
        val textLeft = compoundPaddingLeft + (layout?.getLineLeft(0) ?: 0f).roundToInt()
        val left = textLeft - iconGapPx - iconPx
        val top = (height - iconPx) / 2
        drawable.setBounds(left, top, left + iconPx, top + iconPx)
        drawable.draw(canvas)
    }
}

/**
 * A line box of exactly [px], as CSS makes one: the spare room, or the
 * shortfall, is split evenly above and below the font's own box.
 */
private class ExactLine(private val px: Int) : LineHeightSpan {
    override fun chooseHeight(
        text: CharSequence,
        start: Int,
        end: Int,
        spanstartv: Int,
        lineHeight: Int,
        fm: Paint.FontMetricsInt,
    ) {
        val natural = fm.descent - fm.ascent
        if (natural <= 0) {
            return
        }
        val spare = px - natural
        val above = spare / 2
        fm.ascent -= above
        fm.descent += spare - above
    }
}

/**
 * A white box with solid rules along some of its edges, drawn inside its
 * bounds: the app bar's black line, a row's hairline, the notice's bar.
 */
internal class Edges(private val color: Int, private val widthPx: Int, private val edges: Int) : Drawable() {
    private val paint = Paint()

    override fun draw(canvas: Canvas) {
        val b = bounds
        paint.color = Ink.WHITE
        canvas.drawRect(b, paint)
        paint.color = color
        val left = b.left.toFloat()
        val top = b.top.toFloat()
        val right = b.right.toFloat()
        val bottom = b.bottom.toFloat()
        if (edges and TOP != 0) {
            canvas.drawRect(left, top, right, top + widthPx, paint)
        }
        if (edges and BOTTOM != 0) {
            canvas.drawRect(left, bottom - widthPx, right, bottom, paint)
        }
        if (edges and LEFT != 0) {
            canvas.drawRect(left, top, left + widthPx, bottom, paint)
        }
    }

    override fun setAlpha(alpha: Int) = Unit

    override fun setColorFilter(colorFilter: ColorFilter?) = Unit

    @Deprecated("Deprecated in Java")
    override fun getOpacity(): Int = PixelFormat.OPAQUE

    companion object {
        const val TOP = 1
        const val BOTTOM = 2
        const val LEFT = 4
    }
}

/**
 * The footer of [Widgets.footer]. In a tablet's right column it has no rule
 * and 16 dp under its buttons, as frame 10 draws it. The navigator says
 * where it is.
 */
internal class FooterBar(context: Context) : LinearLayout(context) {
    var inColumn: Boolean = false
        set(value) {
            field = value
            look()
        }

    init {
        orientation = VERTICAL
        look()
    }

    private fun look() {
        background = if (inColumn) {
            ColorDrawable(Ink.WHITE)
        } else {
            Edges(Ink.BLACK, Ink.dp(context, 1f), Edges.TOP)
        }
        setPadding(paddingLeft, Ink.dp(context, 12f), paddingRight, Ink.dp(context, if (inColumn) 16f else 12f))
    }
}

/**
 * A frame no wider than [maxPx]: the dialog on a tablet, and the gate's
 * single column (D3). Narrower screens give it their own width.
 */
internal class CappedWidth(context: Context, private val maxPx: Int) : FrameLayout(context) {
    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val mode = MeasureSpec.getMode(widthMeasureSpec)
        val size = MeasureSpec.getSize(widthMeasureSpec)
        val spec = when {
            mode == MeasureSpec.UNSPECIFIED -> MeasureSpec.makeMeasureSpec(maxPx, MeasureSpec.AT_MOST)
            size > maxPx -> MeasureSpec.makeMeasureSpec(maxPx, mode)
            else -> widthMeasureSpec
        }
        super.onMeasure(spec, heightMeasureSpec)
    }
}

private fun dp(context: Context, value: Float): Int = Ink.dp(context, value)

/** Whole pixels for [value] sp, the unit of a line height as of a font size. */
private fun sp(context: Context, value: Float): Int =
    TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_SP, value, context.resources.displayMetrics).roundToInt()

/** The line CSS calls normal for this font and size, rounded to a whole pixel as the design rounds it. */
private fun naturalLine(sizeSp: Float, mono: Boolean): Float =
    (sizeSp * if (mono) MONO_LINE else INTER_LINE).roundToInt().toFloat()

/** A white box with an outline drawn inside its bounds. */
private fun box(context: Context, fill: Int, stroke: Int, strokeDp: Float): GradientDrawable =
    GradientDrawable().apply {
        setColor(fill)
        setStroke(dp(context, strokeDp), stroke)
    }

/** Room between the children of a LinearLayout, skipped for a child that is gone. */
private fun gap(context: Context, gapDp: Float): Drawable = GradientDrawable().apply {
    val size = dp(context, gapDp)
    setSize(size, size)
}

private fun column(context: Context, gapDp: Float): LinearLayout = LinearLayout(context).apply {
    orientation = LinearLayout.VERTICAL
    dividerDrawable = gap(context, gapDp)
    showDividers = LinearLayout.SHOW_DIVIDER_MIDDLE
}

private fun row(context: Context, gapDp: Float): LinearLayout = LinearLayout(context).apply {
    orientation = LinearLayout.HORIZONTAL
    isBaselineAligned = false
    dividerDrawable = gap(context, gapDp)
    showDividers = LinearLayout.SHOW_DIVIDER_MIDDLE
}
