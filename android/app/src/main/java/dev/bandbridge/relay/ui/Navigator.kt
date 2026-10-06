package dev.bandbridge.relay.ui

import android.app.Activity
import android.content.Context
import android.graphics.Rect
import android.os.Bundle
import android.os.SystemClock
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction
import android.view.inputmethod.InputMethodManager
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ScrollView

private const val MATCH = ViewGroup.LayoutParams.MATCH_PARENT
private const val WRAP = ViewGroup.LayoutParams.WRAP_CONTENT

/** From this width the pages lay out in two columns (D3). */
private const val WIDE_DP = 900

/** The tablet's right column: the rules, or the page on top of them (frames 09 and 10). */
private const val RIGHT_COLUMN_DP = 560f

/** The gate's single column on a tablet, which the design does not draw (D3). */
private const val GATE_COLUMN_DP = 640f

/** Room at the left and right of every page: frame 02 on a phone, frame 09 on a tablet. */
private const val PHONE_GUTTER_DP = 16f
private const val TABLET_GUTTER_DP = 24f

/** How long taps are ignored after the screen changes under the finger (see [TapGuard]). */
private const val QUIET_MS = 500L

/** Where the app can be. Each one is a page on the navigator's stack. */
sealed interface Screen {
    /** The events and the rules (frames 02, 03, 04 and 09). */
    data object Main : Screen

    /**
     * The rule at [at] in the list, or a new rule when [at] is null, with
     * [prefill] as its signal (frames 05 to 07 and 10).
     */
    data class RuleEditor(val at: Int?, val prefill: String?) : Screen

    /** Setup (frames 01 and 08): the root while an app is missing, or reached from the main page. */
    data class Setup(val asRoot: Boolean) : Screen

    /** The connection check (frames 11 to 15). */
    data object Check : Screen
}

/**
 * One page of the app, as the navigator shows it.
 *
 * A page builds its views once and hands the same ones back on every call.
 * The navigator moves them between its phone and tablet layouts rather than
 * asking for new ones, so a page keeps what the person typed or scrolled. A
 * page sets the top and bottom padding of its body and footer, and the
 * navigator sets the left and right: 16 dp on a phone, 24 dp on a tablet.
 * A page's constructor runs while the stack changes, so it must not
 * navigate, and must not read navigator.top or navigator.root: the first
 * page is built before the stack holds any entry.
 */
interface Page {
    /** App bar title on a phone, column header title on a tablet. */
    val title: String

    /** App bar status on a phone: "Listening" on Main, "Not set up" on the gate. */
    val status: String? get() = null

    /** False only for Setup as the gate root. */
    val canGoBack: Boolean get() = true

    /**
     * The scrolling content. Built once: the same view comes back on every call.
     *
     * The navigator stretches it to at least the height it has room for, so
     * a page can push a view to the bottom with a weighted spacer, as frame
     * 05 does with its note.
     */
    fun body(): View

    /** Pinned under the body, such as Save. Built once, like the body. */
    fun footer(): View? = null

    /**
     * D3: the navigator is about to lay the page out in a tablet's right
     * column, or on its own. Frame 10 draws the editor tighter in the column
     * than frame 05 draws it on a phone. It runs before every layout, a
     * rotation's too, and changes only how the page looks.
     */
    fun place(inColumn: Boolean) {}

    /** D16: the page became the visible top. */
    fun enter() {}

    /** D16: the page stopped being the visible top. Disarm and stop timers here. */
    fun leave() {}

    /** D16: stored data changed. Redraw from storage, and never change the page's mode. */
    fun refresh() {}
}

/**
 * Main, which a tablet splits into two columns. body() is the phone's
 * stacked form of the same views.
 *
 * On a tablet the navigator lends the two columns to its own layout. Before
 * the phone layout shows body() again, it puts each column back where it
 * found it, with the padding it had. So body() is simply a view that holds
 * both columns, built once like any other body.
 */
interface SplitPage : Page {
    fun eventsColumn(): View
    fun rulesColumn(): View
}

/**
 * The scroll around a page's body or column. It stretches the content to its
 * own height, never glows or springs at an end, shows no scrollbar, which
 * would fade, and moves in one step, never in a glide: nothing on an e-ink
 * screen may animate.
 *
 * Turning smooth scrolling off stops ScrollView's own glides: for the arrow
 * and page keys, and for a field the keyboard would cover. Two paths glide
 * past that setting, because they call smoothScrollBy or smoothScrollTo
 * themselves, and both jump here instead. A field keeps its cursor in view
 * by asking for its rectangle with immediate false, so a field that grows a
 * line past the visible edge would glide the page down. A screen reader's
 * scroll forward and back would glide a page at a time.
 */
internal class PageScroll(context: Context) : ScrollView(context) {
    init {
        isFillViewport = true
        overScrollMode = OVER_SCROLL_NEVER
        isVerticalScrollBarEnabled = false
        isSmoothScrollingEnabled = false
    }

    /**
     * A field keeps its cursor in view through here with immediate false,
     * and ScrollView would then glide, whatever its smooth scrolling says.
     * It always jumps instead.
     */
    override fun requestChildRectangleOnScreen(child: View, rectangle: Rect, immediate: Boolean): Boolean =
        super.requestChildRectangleOnScreen(child, rectangle, true)

    /** A page forward or back, as ScrollView computes it, in one step. */
    override fun performAccessibilityAction(action: Int, arguments: Bundle?): Boolean {
        val direction = when (action) {
            AccessibilityNodeInfo.ACTION_SCROLL_FORWARD, AccessibilityAction.ACTION_SCROLL_DOWN.id -> 1
            AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD, AccessibilityAction.ACTION_SCROLL_UP.id -> -1
            else -> return super.performAccessibilityAction(action, arguments)
        }
        if (!isEnabled) {
            return false
        }
        val viewport = height - paddingTop - paddingBottom
        val range = maxOf(0, (getChildAt(0)?.height ?: 0) - viewport)
        val target = (scrollY + direction * viewport).coerceIn(0, range)
        if (target == scrollY) {
            return false
        }
        scrollTo(0, target)
        return true
    }
}

/**
 * The navigator's own view: the pages and any dialog or menu over them.
 * Every touch passes its [guard] first, so one that the guard ignores
 * reaches neither a page nor what floats over it.
 */
private class GuardedHost(context: Context, private val guard: TapGuard) : FrameLayout(context) {
    override fun dispatchTouchEvent(event: MotionEvent): Boolean {
        if (!guard.lets(event.actionMasked == MotionEvent.ACTION_DOWN, event.eventTime)) {
            return true
        }
        return super.dispatchTouchEvent(event)
    }
}

/**
 * Which touches go through: none for [QUIET_MS] after the screen changes
 * under the finger.
 *
 * An e-ink panel takes a moment to draw a change, and invites a second tap
 * meanwhile. That tap lands on whatever the new screen has in the same spot:
 * the editor's Learn button under "Add a rule", the next rule under a closed
 * dialog's Remove, or the Remove of a dialog that just opened under the × of
 * the first rule, which removed that rule unread. A gesture is judged by its
 * first touch, so a drag that starts in the quiet time is ignored to its end,
 * and one already under way goes on. Times are those of MotionEvent, passed
 * in, so the rule is tested without a device.
 */
internal class TapGuard {
    private var quietUntil = Long.MIN_VALUE
    private var ignoring = false

    /**
     * The screen changed [at] this time: a page was shown, popped or
     * replaced, a dialog or a menu opened or closed, or a page changed in
     * place.
     */
    fun changed(at: Long) {
        quietUntil = at + QUIET_MS
    }

    /**
     * Whether a touch event goes through. [down] marks the first touch of a
     * gesture, and [at] is its time.
     *
     * A touch from before the change is ignored too. It waited in the queue
     * while the new screen was built, and was aimed at the old one.
     */
    fun lets(down: Boolean, at: Long): Boolean {
        if (down) {
            ignoring = at < quietUntil
        }
        return !ignoring
    }
}

/**
 * The pages on the navigator's stack and the D16 lifecycle, apart from any
 * view, so the lifecycle is tested without a device.
 *
 * One page at most is the visible top, and only while the app is in front.
 * [Page.enter] runs when a page becomes that, and [Page.leave] when it
 * stops, once each. A page that armed Learn or the check is left by every
 * way off it: covered, popped, replaced, or sent to the background. Without
 * that, every later press would be swallowed with nothing on screen to say
 * why.
 */
internal class PageStack(private val build: (Screen) -> Page, private val changed: () -> Unit) {
    /** One page on the stack, and the screen it shows. */
    class Entry(val screen: Screen, val page: Page)

    private val entries = ArrayList<Entry>()
    private var resumed = false
    private var entered: Entry? = null

    val top: Entry get() = entries.last()
    val root: Entry get() = entries.first()
    val size: Int get() = entries.size
    val all: List<Entry> get() = entries

    /** Builds the first root. It is entered once the app is in front. */
    fun start(first: Screen) {
        entries += Entry(first, build(first))
        changed()
        enterTop()
    }

    /**
     * Makes [screen] the top, and keeps one page per screen on the stack.
     *
     * A new screen is built on top: the top is left, then the new page is
     * entered. The top itself stays as it is. A screen lower on the stack is
     * returned to, as Back returns. A tablet keeps Main's events and its
     * Setup row live beside the right column, and without this each tap on
     * them would stack another copy, each wanting a Back of its own.
     *
     * An editor opened while an editor is the top replaces it. Editors
     * differ by value, so "Add a rule for bb03" in a tablet's events column
     * would otherwise stack a second editor over an open "New rule". The
     * editor it replaces leaves without saving, as Back leaves it, and the
     * page under it is not entered in between.
     */
    fun show(screen: Screen) {
        if (top.screen == screen) {
            return
        }
        val at = entries.indexOfFirst { it.screen == screen }
        if (at >= 0) {
            returnTo(at)
            return
        }
        leaveTop()
        if (screen is Screen.RuleEditor && top.screen is Screen.RuleEditor) {
            entries.removeAt(entries.lastIndex)
        }
        entries += Entry(screen, build(screen))
        changed()
        enterTop()
    }

    /** False at the root. Otherwise it returns to the page under the top. */
    fun pop(): Boolean {
        if (entries.size <= 1) {
            return false
        }
        returnTo(entries.lastIndex - 1)
        return true
    }

    /** The top is left, and the new root is built, alone on the stack, then entered. */
    fun replaceRoot(screen: Screen) {
        leaveTop()
        val page = build(screen)
        entries.clear()
        entries += Entry(screen, page)
        changed()
        enterTop()
    }

    /** Every page on the stack, the covered ones too, since a tablet shows Main beside the top. */
    fun refresh() {
        for (entry in entries.toList()) {
            entry.page.refresh()
        }
    }

    fun pause() {
        leaveTop()
        resumed = false
    }

    fun resume() {
        resumed = true
        enterTop()
    }

    /**
     * The top is left once, every page above [index] is dropped, and the
     * page at [index] is refreshed, since what it shows may have changed
     * while it was covered, then entered.
     */
    private fun returnTo(index: Int) {
        leaveTop()
        while (entries.lastIndex > index) {
            entries.removeAt(entries.lastIndex)
        }
        changed()
        top.page.refresh()
        enterTop()
    }

    private fun enterTop() {
        if (!resumed || entered != null) {
            return
        }
        val entry = top
        entered = entry
        entry.page.enter()
    }

    private fun leaveTop() {
        val entry = entered ?: return
        entered = null
        entry.page.leave()
    }
}

/**
 * The pages of the one window: a stack of them, laid out for a phone or,
 * from 900 dp wide, for a tablet (D2, D3), and told when they become or stop
 * being the visible top (D16).
 *
 * On a phone the top page fills the window: its app bar, its body in a
 * scroll, and its footer pinned. On a tablet with Main at the root, Main's
 * app bar spans the top and its footer, the Setup row, the bottom. Between
 * them, Main's events fill the left and a 560 dp column on the right holds
 * the rules, or the top page under a column header. Any other root, which
 * is the gate, is one centered column. A rotation never recreates the
 * Activity (D2): [relayout] moves the same page views into the new layout.
 *
 * For half a second after every layout, after a dialog or a menu opens or
 * closes, and after a page says it changed in place, taps are ignored (see
 * [TapGuard]), so a second tap on an e-ink panel cannot land on what the
 * change put under the finger.
 */
class Navigator(
    private val activity: Activity,
    root: FrameLayout,
    first: Screen,
    private val pages: (Screen, Navigator) -> Page,
) {
    /** A navigator whose first root is Main. */
    constructor(activity: Activity, root: FrameLayout, pages: (Screen, Navigator) -> Page) :
        this(activity, root, Screen.Main, pages)

    private val guard = TapGuard()

    /**
     * Where a dialog or a menu goes, over every page: pass it to
     * [Widgets.confirmDialog] or [Widgets.menu]. Nothing else goes here,
     * because a view added here hides the pages from a screen reader until
     * it is removed, and starts the quiet time of the tap guard, as its
     * removal does.
     */
    val host: FrameLayout = GuardedHost(activity, guard)

    /** Holds the pages' layout. A dialog or a menu sits above it in [host], so a relayout keeps an open one. */
    private val frame = FrameLayout(activity).apply {
        // It takes the focus itself, so that no field takes it, and opens
        // the keyboard, when a page appears.
        isFocusableInTouchMode = true
        descendantFocusability = ViewGroup.FOCUS_BEFORE_DESCENDANTS
        defaultFocusHighlightEnabled = false
    }

    /** One scroll per page part, kept while the page is on the stack, so its scroll position survives. */
    private val scrolls = HashMap<Pair<PageStack.Entry, Part>, ScrollView>()

    /** Main's columns, while the tablet layout has them, and where each came from. */
    private val loans = ArrayList<Loan>()

    private val stack = PageStack(build = { pages(it, this) }, changed = ::layout)

    /** 900 dp or wider: a tablet, or a phone on its side (D3). */
    val wide: Boolean
        get() = activity.resources.configuration.screenWidthDp >= WIDE_DP

    val top: Screen
        get() = stack.top.screen

    val root: Screen
        get() = stack.root.screen

    init {
        root.addView(host, FrameLayout.LayoutParams(MATCH, MATCH))
        host.addView(frame, 0, FrameLayout.LayoutParams(MATCH, MATCH))
        host.setOnHierarchyChangeListener(object : ViewGroup.OnHierarchyChangeListener {
            override fun onChildViewAdded(parent: View, child: View) = floatingChanged(child, open = true)

            override fun onChildViewRemoved(parent: View, child: View) = floatingChanged(child, open = false)
        })
        stack.start(first)
    }

    /**
     * Covers the top with a new page for [screen]: top.leave(), push, new
     * top.enter(). A screen already on top stays as it is, and one lower on
     * the stack is returned to as Back returns, so no screen is ever on the
     * stack twice. An editor opened over an editor replaces it, and the one
     * replaced leaves without saving, as Back leaves it. An open dialog or
     * menu closes.
     */
    fun show(screen: Screen) {
        if (screen == top) {
            return
        }
        Widgets.closeFloating(host)
        stack.show(screen)
    }

    /**
     * System Back and the arrow. An open dialog or menu closes first. False
     * at the root, where Back leaves the app. Otherwise top.leave(), pop,
     * then the uncovered page's refresh() and enter().
     */
    fun back(): Boolean {
        if (Widgets.closeFloating(host)) {
            return true
        }
        return stack.pop()
    }

    /** The gate's swap: top.leave(), clear the stack, new root.enter(). Nothing is stacked under it. */
    fun replaceRoot(screen: Screen) {
        Widgets.closeFloating(host)
        stack.replaceRoot(screen)
    }

    /** refresh() on every page on the stack. */
    fun refresh() = stack.refresh()

    /**
     * The page on the stack for [screen], or null when the stack does not
     * hold it. The window hands a page what another app sent back to it:
     * Main gets the file the person chose in Android's Save or Open dialog.
     */
    fun pageOf(screen: Screen): Page? = stack.all.firstOrNull { it.screen == screen }?.page

    /** top.leave(), from Activity.onPause. */
    fun pause() = stack.pause()

    /** top.enter(), from Activity.onResume. */
    fun resume() = stack.resume()

    /** Lays the same pages out again after a configuration change, such as a rotation. No page is left or entered. */
    fun relayout() = layout()

    /**
     * The top page changed what it shows in place, with no page change:
     * taps are ignored for half a second, as after one (see [TapGuard]).
     *
     * The navigator sees a layout or a dialog come and go, but not a page
     * whose content moves by itself or after a tap. A page calls this then.
     * A Learn round that ends by itself puts frame 07's notice where Cancel
     * was, and the check's results replace each other where their buttons
     * were: frame 14's "Back to setup steps" lies where frame 13's "Send a
     * test event" was. A second tap on an e-ink panel would otherwise act
     * on what the panel has not yet drawn.
     */
    fun changedInPlace() = guard.changed(SystemClock.uptimeMillis())

    /**
     * Takes the focus from a field back to the navigator's frame, and closes
     * the keyboard, as a page that appears does. A page calls this when it
     * stops wanting typing, as Learn does when it starts. A field that kept
     * the focus would go on blinking its cursor, and on e-ink each blink
     * redraws the panel.
     */
    fun takeFocus() {
        hideKeyboard()
        frame.requestFocus()
    }

    private fun layout() {
        guard.changed(SystemClock.uptimeMillis())
        hideKeyboard()
        returnLoans()
        frame.removeAllViews()
        val gutter = Ink.dp(activity, if (wide) TABLET_GUTTER_DP else PHONE_GUTTER_DP)
        val split = stack.root.page as? SplitPage
        if (wide && split != null) {
            frame.addView(splitLayout(split, gutter), FrameLayout.LayoutParams(MATCH, MATCH))
        } else if (wide) {
            val column = CappedWidth(activity, Ink.dp(activity, GATE_COLUMN_DP))
            column.addView(singleLayout(gutter), FrameLayout.LayoutParams(MATCH, MATCH))
            frame.addView(column, FrameLayout.LayoutParams(MATCH, MATCH, Gravity.CENTER_HORIZONTAL))
        } else {
            frame.addView(singleLayout(gutter), FrameLayout.LayoutParams(MATCH, MATCH))
        }
        scrolls.keys.retainAll { (entry, _) -> stack.all.any { it === entry } }
        frame.requestFocus()
    }

    /** The top page alone: its app bar, its body, and its footer. */
    private fun singleLayout(gutter: Int): View {
        val entry = stack.top
        val page = entry.page
        if (page is SplitPage) {
            // A column the tablet layout had but never borrowed from body()
            // must be free before body() takes it.
            freeFromScroll(page.eventsColumn())
            freeFromScroll(page.rulesColumn())
        }
        val onBack: (() -> Unit)? = if (page.canGoBack && stack.size > 1) {
            { back() }
        } else {
            null
        }
        page.place(inColumn = false)
        return LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            addView(placed(Widgets.appBar(activity, page.title, page.status, onBack), gutter))
            addView(scrolled(entry, Part.BODY, page.body(), gutter), LinearLayout.LayoutParams(MATCH, 0, 1f))
            page.footer()?.let { addView(placed(it, gutter), LinearLayout.LayoutParams(MATCH, WRAP)) }
        }
    }

    /** Main split in two, with the top page in the right column when it is not Main (frames 09 and 10). */
    private fun splitLayout(split: SplitPage, gutter: Int): View {
        val main = stack.root
        val covered = stack.size > 1
        val right = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            if (covered) {
                val top = stack.top
                top.page.place(inColumn = true)
                addView(placed(Widgets.columnHeader(activity, top.page.title) { back() }, gutter))
                addView(scrolled(top, Part.BODY, top.page.body(), gutter), LinearLayout.LayoutParams(MATCH, 0, 1f))
                top.page.footer()?.let {
                    addView(placed(it, gutter, inColumn = true), LinearLayout.LayoutParams(MATCH, WRAP))
                }
            } else {
                addView(
                    scrolled(main, Part.RULES, lend(split.rulesColumn()), gutter),
                    LinearLayout.LayoutParams(MATCH, 0, 1f),
                )
            }
        }
        val columns = LinearLayout(activity).apply {
            orientation = LinearLayout.HORIZONTAL
            addView(
                scrolled(main, Part.EVENTS, lend(split.eventsColumn()), gutter),
                LinearLayout.LayoutParams(0, MATCH, 1f),
            )
            // The rule is 1 dp beside the rules and 2 dp beside an open page, as frames 09 and 10 draw it.
            addView(
                View(activity).apply { setBackgroundColor(Ink.BLACK) },
                LinearLayout.LayoutParams(Ink.dp(activity, if (covered) 2f else 1f), MATCH),
            )
            addView(right, LinearLayout.LayoutParams(Ink.dp(activity, RIGHT_COLUMN_DP), MATCH))
        }
        return LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            addView(placed(Widgets.appBar(activity, main.page.title, main.page.status, null), gutter))
            addView(columns, LinearLayout.LayoutParams(MATCH, 0, 1f))
            main.page.footer()?.let { addView(placed(it, gutter), LinearLayout.LayoutParams(MATCH, WRAP)) }
        }
    }

    /** [content] in the page part's own [PageScroll], with the gutter at its sides. */
    private fun scrolled(entry: PageStack.Entry, part: Part, content: View, gutter: Int): ScrollView {
        val scroll = scrolls.getOrPut(entry to part) { PageScroll(activity) }
        if (content.parent !== scroll) {
            detach(content)
            scroll.removeAllViews()
            scroll.addView(content, FrameLayout.LayoutParams(MATCH, WRAP))
        }
        content.setPadding(gutter, content.paddingTop, gutter, content.paddingBottom)
        detach(scroll)
        return scroll
    }

    /** A page's view, or a bar, taken from where it was and given the gutter. A footer learns whether it is in the column. */
    private fun placed(view: View, gutter: Int, inColumn: Boolean = false): View {
        detach(view)
        (view as? FooterBar)?.inColumn = inColumn
        view.setPadding(gutter, view.paddingTop, gutter, view.paddingBottom)
        return view
    }

    /** Takes one of Main's columns out of Main's body for the tablet layout, remembering where it was. */
    private fun lend(column: View): View {
        val home = column.parent as? ViewGroup
        if (home != null && scrolls.values.none { it === home }) {
            loans += Loan(column, home, home.indexOfChild(column), column.layoutParams, column.paddingLeft, column.paddingRight)
            home.removeView(column)
        }
        return column
    }

    /** Puts each lent column back where it came from, the last lent first, so every index is right. */
    private fun returnLoans() {
        while (loans.isNotEmpty()) {
            val loan = loans.removeAt(loans.lastIndex)
            detach(loan.column)
            loan.column.setPadding(loan.left, loan.column.paddingTop, loan.right, loan.column.paddingBottom)
            loan.home.addView(loan.column, loan.index.coerceIn(0, loan.home.childCount), loan.params)
        }
    }

    private fun freeFromScroll(view: View) {
        val parent = view.parent
        if (scrolls.values.any { it === parent }) {
            detach(view)
        }
    }

    private fun detach(view: View) {
        (view.parent as? ViewGroup)?.removeView(view)
    }

    /**
     * A dialog or a menu opened or closed, and the screen changed under the
     * finger, so the tap guard's quiet time starts.
     *
     * While one is open, a screen reader reaches only it, as a sighted
     * person does: the pages under it are hidden from the reader, and shown
     * again when it closes. Without this it could still reach a rule's × or
     * the Setup row through the dialog's scrim, or open a rule under the
     * menu, whose touches only close the menu.
     *
     * Every view the host holds besides the frame is a dialog or a menu, and
     * at most one is open, so one added covers the pages and one removed
     * uncovers them.
     */
    private fun floatingChanged(child: View, open: Boolean) {
        if (child === frame) {
            return
        }
        guard.changed(SystemClock.uptimeMillis())
        frame.importantForAccessibility = if (open) {
            View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
        } else {
            View.IMPORTANT_FOR_ACCESSIBILITY_AUTO
        }
    }

    /** A page that is left takes the keyboard with it. */
    private fun hideKeyboard() {
        val token = frame.windowToken ?: return
        activity.getSystemService(InputMethodManager::class.java)?.hideSoftInputFromWindow(token, 0)
    }

    /** The parts of a page that scroll on their own. */
    private enum class Part { BODY, EVENTS, RULES }

    /** A column lent to the tablet layout: its home, its place there, and the padding it had. */
    private class Loan(
        val column: View,
        val home: ViewGroup,
        val index: Int,
        val params: ViewGroup.LayoutParams,
        val left: Int,
        val right: Int,
    )
}
