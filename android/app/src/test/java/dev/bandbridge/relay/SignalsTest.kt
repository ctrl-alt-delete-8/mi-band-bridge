package dev.bandbridge.relay

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SignalsTest {
    private val rules = listOf(
        Rule("bb01", "bb01", "uid-one"),
        Rule("bb02", "bb02", "uid-two"),
        Rule("bb03", "bb03", ""),
    )

    private fun uids(payload: String, rules: List<Rule>) =
        Signals.matchAll(payload, rules).map { it.keyMapUid }

    /** The handle byte differs between presses, so a match must not depend on it. */
    @Test
    fun matchesWhateverTheHandleIs() {
        assertEquals(listOf("uid-one"), uids("0303004F0002000000A000BB01", rules))
        assertEquals(listOf("uid-one"), uids("030300010002000000A000BB01", rules))
        assertEquals(listOf("uid-two"), uids("0303001C0002000000A000BB02", rules))
    }

    /** Gadgetbridge writes upper case, but a person typing a rule may not. */
    @Test
    fun theCaseOfEitherSideDoesNotMatter() {
        val lower = listOf(Rule("bb01", "bb01", "uid-one"))
        assertEquals(listOf("uid-one"), uids("0303004f0002000000a000bb01", lower))
    }

    /** A rule with no key map would fire nothing, so it must not swallow a press. */
    @Test
    fun aRuleWithNoKeyMapDoesNotMatch() {
        assertTrue(uids("0303004F0002000000A000BB03", rules).isEmpty())
    }

    /** Only the band app endpoint carries a signal, so other band traffic cannot match. */
    @Test
    fun otherBandTrafficDoesNotMatch() {
        assertTrue(uids("0303004F00020000001B00BB01", rules).isEmpty())
        assertTrue(uids("0303004F0002000000A000BB09", rules).isEmpty())
        assertTrue(uids("", rules).isEmpty())
    }

    /**
     * One signal may drive several key maps.
     *
     * Each key map carries its own "app in foreground" constraint in Key
     * Mapper, and Key Mapper skips the ones whose constraint fails. So the
     * relay fires all of them and lets Key Mapper choose.
     */
    @Test
    fun oneSignalFiresEveryKeyMapBoundToIt() {
        val perApp = listOf(
            Rule("bb02", "bb02", "reader-page-down"),
            Rule("bb03", "bb03", "reader-page-up"),
            Rule("bb02", "bb02", "browser-scroll-down"),
        )
        assertEquals(
            listOf("reader-page-down", "browser-scroll-down"),
            uids("030700FB0002000000A000BB02", perApp)
        )
    }

    /** A key map listed twice for one signal would run twice on one press. */
    @Test
    fun theSameKeyMapListedTwiceFiresOnce() {
        val doubled = listOf(Rule("bb02", "bb02", "uid-two"), Rule(" BB02 ", "bb02", "uid-two "))
        assertEquals(listOf("uid-two"), uids("030700FB0002000000A000BB02", doubled))
    }

    @Test
    fun aBandAppPressHasItsSignal() {
        assertEquals("bb01", Signals.signalOf("0303004F0002000000A000BB01"))
        assertEquals("bb02", Signals.signalOf("030700FB0002000000A000BB02"))
        assertEquals("bb0401", Signals.signalOf("030300110003000000A000BB0401"))
    }

    /** Other endpoints, other first bytes, encrypted chunks and continuation chunks carry no signal. */
    @Test
    fun otherTrafficHasNoSignal() {
        assertNull(Signals.signalOf("0303004F00020000001B00BB01"))
        assertNull(Signals.signalOf("0303004F0002000000A0000201"))
        assertNull(Signals.signalOf("030B004F0002000000A000BB01"))
        assertNull(Signals.signalOf("0302004F0002000000A000BB01"))
    }

    @Test
    fun onlyABandAppSignalIsAPress() {
        assertEquals(Press.Signal("bb01"), Signals.pressOf("0303004F0002000000A000BB01"))
        assertEquals(Press.Signal("bb01"), Signals.pressOf(TEST_PAYLOAD))
        assertEquals(Press.Signal("bb0401"), Signals.pressOf("030300110003000000A000BB0401"))
    }

    /** The user chose band app presses only: a music key is the band's own traffic. */
    @Test
    fun aMusicKeyIsBandTraffic() {
        for (code in listOf("0500", "0501", "0503", "0504", "0505", "0506")) {
            assertNull(code, Signals.pressOf("030300120002000000" + "1B00" + code))
        }
    }

    @Test
    fun theBandsOwnTrafficIsNoPress() {
        for (traffic in listOf(
            "030B00120002000000" + "1B00" + "0505",   // encrypted
            "0302001201" + "0505",                    // a continuation chunk
            "0400120101",                             // an acknowledgement
            "030300120002000000" + "0100" + "0505",   // the weather endpoint
            "030300120002000000" + "1B00" + "0401",   // the music screen opened
            "030300120002000000" + "A000" + "0201",   // app management on the app endpoint
        )) {
            assertNull(traffic, Signals.pressOf(traffic))
        }
    }

    @Test
    fun malformedPayloadsHaveNoSignalAndDoNotThrow() {
        for (bad in listOf("", "03", "0303004F0002000000A000", "0303004F0009000000A000BB01", "zz03004F0002000000A000BB01", "0303004F0002000000A000BB0")) {
            assertNull(bad, Signals.signalOf(bad))
            Signals.labelOf(bad)
        }
    }

    /** The length is a u32. Read into an Int, one past Int.MAX_VALUE would throw in the receiver. */
    @Test
    fun aLengthPastTheIntRangeIsNoSignal() {
        assertNull(Signals.signalOf("0303004F0000000080A000BB01"))
        assertNull(Signals.signalOf("0303004F00FFFFFFFFA000BB01"))
    }

    @Test
    fun anEventWithNoSignalShowsItsFirstTwoBytes() {
        assertEquals("bb01", Signals.labelOf("0303004F0002000000A000BB01"))
        assertEquals("0303", Signals.labelOf("0303004F00020000001B00BB01"))
        assertEquals("03", Signals.labelOf("03"))
        assertEquals("—", Signals.labelOf(""))
    }

    /** A row stored by 2.2.0 for a music key still draws, with its first two bytes. */
    @Test
    fun anOldMusicRowStillHasALabel() {
        assertEquals("0303", Signals.labelOf("030300120002000000" + "1B00" + "0505"))
    }

    /** The rule src/signal.rs applies on the Mac, so a signal typed here works there too. */
    @Test
    fun aSignalFollowsTheMacRule() {
        assertEquals("bb04", Signals.normalize(" BB04 "))
        assertEquals("bb0401", Signals.normalize("bb0401"))
        for (bad in listOf("", "bb", "0201", "c0ffee", "bbz1", "bb0", "bb0102030405060708")) {
            assertNull(bad, Signals.normalize(bad))
        }
    }

    /** Character.digit alone also reads fullwidth letters and other scripts' digits, which the Mac refuses. */
    @Test
    fun onlyAsciiDigitsAreHex() {
        assertNull(Signals.normalize("ｂｂ01"))
        assertNull(Signals.normalize("bb٠١"))
    }

    /** The length field says where the signal ends, and bytes past it are ignored, as the Mac's decoder does. */
    @Test
    fun theLengthFieldSaysWhereTheSignalEnds() {
        assertEquals("bb01", Signals.signalOf("0303004F0002000000A000BB0102"))
        assertEquals("bb0102", Signals.signalOf("0303004F0003000000A000BB0102"))
    }

    @Test
    fun aPayloadSeenTwiceAtOnceCountsOnce() {
        assertTrue(Signals.isFirstSighting("030300770002000000A000BB01", now = 1_000))
        assertFalse(Signals.isFirstSighting("030300770002000000A000BB01", now = 1_500))
        assertTrue(Signals.isFirstSighting("030300770002000000A000BB01", now = 2_500))
    }

    /** On Android 13 and older the manifest receiver gets a broadcast after the service does, so other traffic can come between. */
    @Test
    fun aPressCountsOnceWhenAnotherEventLandsBetweenItsDeliveries() {
        val press = "030300780002000000A000BB01"
        val other = "0303007900020000001B00BB01"
        assertTrue(Signals.isFirstSighting(press, now = 1_000))
        assertTrue(Signals.isFirstSighting(other, now = 1_010))
        assertFalse(Signals.isFirstSighting(press, now = 1_020))
        assertFalse(Signals.isFirstSighting(other, now = 1_030))
        assertTrue(Signals.isFirstSighting(press, now = 1_900))
    }

    /** A sighting stamped before the clock was set back must not hold its payload until the clock catches up. */
    @Test
    fun aClockSetBackDoesNotHoldAPayload() {
        assertTrue(Signals.isFirstSighting("030300800002000000A000BB01", now = 50_000))
        assertTrue(Signals.isFirstSighting("030300800002000000A000BB01", now = 20_000))
    }
}
