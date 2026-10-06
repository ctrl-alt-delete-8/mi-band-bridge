package dev.bandbridge.relay

import org.junit.Assert.assertEquals
import org.junit.Test

class RulesTest {
    @Test
    fun rulesRoundTripInOrder() {
        val rules = listOf(
            Rule("bb01", "Page down", "3f2a9c1e-7b4d-4e0a-9c61-2d8e5f0a1b3c"),
            Rule("bb01", "Next slide, \"quoted\" = odd", "8d7b2e4a-1c69-4f03-a512-9e0b6c3d8f21"),
        )
        assertEquals(rules, Rules.decode(Rules.encode(rules)))
    }

    /** Without this the band stops working the moment the reader updates the app. */
    @Test
    fun oldRulesSurviveAsNamedRules() {
        assertEquals(
            listOf(Rule("bb01", "bb01", "uid-1"), Rule("bb03", "bb03", "uid-3"), Rule("bb03", "bb03", "uid-4")),
            Rules.migrate("bb01=uid-1\nbb02=\n BB03 =uid-3 \nbb03=uid-4\n"),
        )
        assertEquals(emptyList<Rule>(), Rules.migrate(" "))
    }

    /** D5: a signal the Mac would refuse stays in the list, so the person can see it and fix it. */
    @Test
    fun anOldSignalOutsideTheRuleIsKeptAsTyped() {
        assertEquals(listOf(Rule("c0ffee", "c0ffee", "uid-9")), Rules.migrate(" C0FFEE =uid-9"))
    }

    @Test
    fun aNewRuleGoesLastAndAnEditedOneKeepsItsPlace() {
        val pageDown = Rule("bb01", "Page down", "uid-1")
        val pageUp = Rule("bb02", "Page up", "uid-2")
        val nextSlide = Rule("bb01", "Next slide", "uid-3")
        assertEquals(listOf(pageDown, pageUp, nextSlide), Rules.saved(listOf(pageDown, pageUp), nextSlide, was = null, at = null))
        val renamed = Rule("bb02", "Scroll up", "uid-2")
        assertEquals(listOf(pageDown, renamed), Rules.saved(listOf(pageDown, pageUp), renamed, was = pageUp, at = 1))
    }

    /** Two rules can be alike. The one the editor opened is the one replaced, not the first of them. */
    @Test
    fun anEditedRuleIsReplacedWhereItWasOpened() {
        val same = Rule("bb01", "Page down", "uid-1")
        val changed = Rule("bb01", "Page down", "uid-9")
        assertEquals(listOf(same, changed), Rules.saved(listOf(same, same), changed, was = same, at = 1))
    }

    /**
     * The list can change under an open editor, from a second window. The
     * edited rule is found again rather than another rule overwritten, and
     * if it is gone, the edit is kept as a new rule.
     */
    @Test
    fun anEditedRuleThatMovedIsFoundAgain() {
        val pageDown = Rule("bb01", "Page down", "uid-1")
        val pageUp = Rule("bb02", "Page up", "uid-2")
        val renamed = Rule("bb02", "Scroll up", "uid-2")
        assertEquals(listOf(renamed), Rules.saved(listOf(pageUp), renamed, was = pageUp, at = 1))
        assertEquals(listOf(renamed, pageDown), Rules.saved(listOf(pageUp, pageDown), renamed, was = pageUp, at = 1))
        assertEquals(listOf(pageDown, renamed), Rules.saved(listOf(pageDown), renamed, was = pageUp, at = 1))
    }

    @Test
    fun anUnreadableStoreIsNoRulesNotACrash() {
        assertEquals(emptyList<Rule>(), Rules.decode("not json"))
        assertEquals(emptyList<Rule>(), Rules.decode("[{\"signal\":\"bb01\"}]"))
    }
}
