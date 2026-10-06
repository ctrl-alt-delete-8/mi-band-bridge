package dev.bandbridge.relay.ui

import android.view.View
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The D16 lifecycle, without views: which page is entered and left, and
 * when. Each page writes its calls into one log, and the navigator's layout
 * writes "layout".
 */
class PageStackTest {
    private val log = mutableListOf<String>()

    private val stack = PageStack(build = { Probe(nameOf(it), log) }, changed = { log += "layout" })

    @Test
    fun aPageIsEnteredOnlyOnceTheAppIsInFront() {
        stack.start(Screen.Main)
        assertEquals(listOf("layout"), log)
        stack.resume()
        assertEquals(listOf("layout", "Main enter"), log)
    }

    @Test
    fun showingAPageLeavesTheOneItCovers() {
        running(Screen.Main)
        stack.show(Screen.RuleEditor(at = null, prefill = null))
        assertEquals(listOf("Main leave", "layout", "RuleEditor enter"), log)
    }

    /** The uncovered page may show stale data, such as a rule the editor just saved. */
    @Test
    fun backLeavesTheTopThenRefreshesAndEntersThePageUnderIt() {
        running(Screen.Main)
        stack.show(Screen.Check)
        log.clear()
        assertTrue(stack.pop())
        assertEquals(listOf("Check leave", "layout", "Main refresh", "Main enter"), log)
        assertEquals(Screen.Main, stack.top.screen)
    }

    @Test
    fun backAtTheRootDoesNothing() {
        running(Screen.Main)
        assertFalse(stack.pop())
        assertEquals(emptyList<String>(), log)
    }

    @Test
    fun theBackgroundLeavesTheTopAndTheForegroundEntersItAgain() {
        running(Screen.Main)
        stack.show(Screen.Check)
        log.clear()
        stack.pause()
        stack.resume()
        assertEquals(listOf("Check leave", "Check enter"), log)
    }

    /** onResume swaps the gate's root before resume(), and the new root must be entered once, not twice. */
    @Test
    fun aRootSwappedWhileInTheBackgroundIsEnteredOnceOnResume() {
        running(Screen.Setup(asRoot = true))
        stack.pause()
        log.clear()
        stack.replaceRoot(Screen.Main)
        stack.resume()
        assertEquals(listOf("layout", "Main enter"), log)
        assertEquals(1, stack.size)
    }

    @Test
    fun replacingTheRootLeavesTheTopAndStacksNothingUnderTheNewRoot() {
        running(Screen.Setup(asRoot = true))
        stack.show(Screen.Check)
        log.clear()
        stack.replaceRoot(Screen.Main)
        assertEquals(listOf("Check leave", "layout", "Main enter"), log)
        assertEquals(listOf<Screen>(Screen.Main), stack.all.map { it.screen })
    }

    @Test
    fun aPageShownWhileInTheBackgroundWaitsForTheForeground() {
        running(Screen.Main)
        stack.pause()
        log.clear()
        stack.show(Screen.Check)
        assertEquals(listOf("layout"), log)
        stack.resume()
        assertEquals(listOf("layout", "Check enter"), log)
    }

    /**
     * Review Focus 2: a page that armed Learn or the check is left by every
     * way off it. Covered, sent to the background, popped and replaced, then
     * the app goes to the background: every enter has had its leave.
     */
    @Test
    fun everyWayOffAPageLeavesItAsOftenAsItWasEntered() {
        stack.start(Screen.Main)
        stack.resume()
        stack.show(Screen.RuleEditor(at = 0, prefill = null))
        stack.pause()
        stack.resume()
        stack.show(Screen.Setup(asRoot = false))
        assertTrue(stack.pop())
        assertTrue(stack.pop())
        stack.show(Screen.Check)
        stack.replaceRoot(Screen.Setup(asRoot = true))
        stack.pause()
        for (name in listOf("Main", "RuleEditor", "Setup", "Check")) {
            val entered = log.count { it == "$name enter" }
            assertTrue(name, entered > 0)
            assertEquals(name, entered, log.count { it == "$name leave" })
        }
    }

    /** A tablet keeps Main's Setup row live beside Setup itself, and a second tap must not stack a copy. */
    @Test
    fun showingTheTopAgainDoesNothing() {
        running(Screen.Main)
        stack.show(Screen.Setup(asRoot = false))
        log.clear()
        stack.show(Screen.Setup(asRoot = false))
        assertEquals(emptyList<String>(), log)
        assertEquals(listOf<Screen>(Screen.Main, Screen.Setup(asRoot = false)), stack.all.map { it.screen })
    }

    /** As Back returns: the top leaves once, then the page returned to is refreshed and entered. */
    @Test
    fun showingAScreenLowerOnTheStackReturnsToIt() {
        running(Screen.Main)
        stack.show(Screen.Setup(asRoot = false))
        stack.show(Screen.Check)
        log.clear()
        stack.show(Screen.Setup(asRoot = false))
        assertEquals(listOf("Check leave", "layout", "Setup refresh", "Setup enter"), log)
        assertEquals(listOf<Screen>(Screen.Main, Screen.Setup(asRoot = false)), stack.all.map { it.screen })
    }

    @Test
    fun showingTheRootReturnsToIt() {
        running(Screen.Main)
        stack.show(Screen.Setup(asRoot = false))
        stack.show(Screen.Check)
        log.clear()
        stack.show(Screen.Main)
        assertEquals(listOf("Check leave", "layout", "Main refresh", "Main enter"), log)
        assertEquals(1, stack.size)
    }

    /** The review's tablet walk: the Setup row twice, Start check, the Setup row again. Then one Back per page. */
    @Test
    fun theTabletWalkTakesOneBackPerPage() {
        running(Screen.Main)
        stack.show(Screen.Setup(asRoot = false))
        stack.show(Screen.Setup(asRoot = false))
        stack.show(Screen.Check)
        stack.show(Screen.Setup(asRoot = false))
        assertTrue(stack.pop())
        assertEquals(Screen.Main, stack.top.screen)
        assertFalse(stack.pop())
        assertEquals(1, log.count { it == "Check enter" })
        assertEquals(1, log.count { it == "Check leave" })
    }

    /**
     * A tablet keeps the events live beside an open editor. "Add a rule for
     * bb03" there replaces "New rule" rather than stacking a second editor,
     * and the editor it replaces leaves without saving, as Back leaves it.
     */
    @Test
    fun anEditorOpenedOverAnEditorReplacesIt() {
        running(Screen.Main)
        stack.show(Screen.RuleEditor(at = null, prefill = null))
        log.clear()
        stack.show(Screen.RuleEditor(at = null, prefill = "bb03"))
        assertEquals(listOf("RuleEditor leave", "layout", "RuleEditor bb03 enter"), log)
        assertEquals(
            listOf<Screen>(Screen.Main, Screen.RuleEditor(at = null, prefill = "bb03")),
            stack.all.map { it.screen },
        )
        assertTrue(stack.pop())
        assertFalse(stack.pop())
    }

    /** Only an editor on top is replaced. Over Setup the editor stacks, and Back returns to Setup. */
    @Test
    fun anEditorOpenedOverAnotherPageStacks() {
        running(Screen.Main)
        stack.show(Screen.Setup(asRoot = false))
        log.clear()
        stack.show(Screen.RuleEditor(at = null, prefill = "bb03"))
        assertEquals(listOf("Setup leave", "layout", "RuleEditor bb03 enter"), log)
        assertTrue(stack.pop())
        assertEquals(Screen.Setup(asRoot = false), stack.top.screen)
    }

    /**
     * No screen is on the stack twice. With an editor for bb03 under Setup
     * and one for bb05 on top, "Add a rule for bb03" returns to the first,
     * as Back would, rather than replacing the top with a copy of it.
     */
    @Test
    fun anEditorAlreadyOnTheStackIsReturnedTo() {
        running(Screen.Main)
        stack.show(Screen.RuleEditor(at = null, prefill = "bb03"))
        stack.show(Screen.Setup(asRoot = false))
        stack.show(Screen.RuleEditor(at = null, prefill = "bb05"))
        log.clear()
        stack.show(Screen.RuleEditor(at = null, prefill = "bb03"))
        assertEquals(listOf("RuleEditor bb05 leave", "layout", "RuleEditor bb03 refresh", "RuleEditor bb03 enter"), log)
        assertEquals(
            listOf<Screen>(Screen.Main, Screen.RuleEditor(at = null, prefill = "bb03")),
            stack.all.map { it.screen },
        )
    }

    /**
     * A page keeps what it shows, such as Setup's copy marks (D13), as long
     * as it has a place on the stack: through a page over it and back, and
     * through the background. Back from it, or a root swapped in its place,
     * drops it, and showing the screen again builds a new page.
     */
    @Test
    fun aPageLastsAsLongAsItsPlaceOnTheStack() {
        running(Screen.Main)
        stack.show(Screen.Setup(asRoot = false))
        val setup = stack.top.page
        stack.show(Screen.Check)
        assertTrue(stack.pop())
        stack.pause()
        stack.resume()
        assertSame(setup, stack.top.page)
        assertTrue(stack.pop())
        stack.show(Screen.Setup(asRoot = false))
        assertNotSame(setup, stack.top.page)

        stack.replaceRoot(Screen.Setup(asRoot = true))
        val gate = stack.top.page
        stack.replaceRoot(Screen.Main)
        stack.replaceRoot(Screen.Setup(asRoot = true))
        assertNotSame(gate, stack.top.page)
    }

    /** A tablet shows Main beside the top page, so a covered page must redraw too. */
    @Test
    fun refreshReachesEveryPageOnTheStackFromTheRootUp() {
        running(Screen.Main)
        stack.show(Screen.Setup(asRoot = false))
        stack.show(Screen.Check)
        log.clear()
        stack.refresh()
        assertEquals(listOf("Main refresh", "Setup refresh", "Check refresh"), log)
    }

    /** A stack started on [first], with the app in front, and an empty log. */
    private fun running(first: Screen) {
        stack.start(first)
        stack.resume()
        log.clear()
    }

    /** An editor with a prefilled signal carries it in its name, so two editors can be told apart. */
    private fun nameOf(screen: Screen): String = when (screen) {
        Screen.Main -> "Main"
        is Screen.RuleEditor -> listOfNotNull("RuleEditor", screen.prefill).joinToString(" ")
        is Screen.Setup -> "Setup"
        Screen.Check -> "Check"
    }

    /** A page that only writes its lifecycle calls down. */
    private class Probe(override val title: String, private val log: MutableList<String>) : Page {
        override fun body(): View = throw UnsupportedOperationException("The stack never asks for a view")

        override fun enter() {
            log += "$title enter"
        }

        override fun leave() {
            log += "$title leave"
        }

        override fun refresh() {
            log += "$title refresh"
        }
    }
}
