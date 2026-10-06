package dev.bandbridge.relay

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.fail
import org.junit.Test

/**
 * Each test uses its own payload handle, or times at least 800 ms apart,
 * because isFirstSighting remembers every payload of the last 800 ms.
 */
class RelayTest {
    private val rules = listOf(Rule("bb01", "Page down", "uid-1"), Rule("bb01", "Next slide", "uid-2"))

    @After
    fun disarm() = Capture.disarm()

    @Test
    fun aPressSendsEveryMatchingRule() {
        val handling = Relay.handle("030300210002000000A000BB01", test = false, now = 10_000, rules = rules)
        assertEquals(Handling.Send(rules), handling)
    }

    @Test
    fun theSecondDeliveryOfAPressDoesNothing() {
        Relay.handle("030300220002000000A000BB01", test = false, now = 20_000, rules = rules)
        assertEquals(Handling.Duplicate, Relay.handle("030300220002000000A000BB01", test = false, now = 20_050, rules = rules))
    }

    @Test
    fun anArmedCaptureTakesThePressAndNoKeyMapRuns() {
        Capture.arm(Capture.Wants.BandSignal) { _, _ -> }
        assertEquals(Handling.Captured, Relay.handle("030300230002000000A000BB01", test = false, now = 30_000, rules = rules))
    }

    /** The test event looks exactly like a bb01 press, and must never act as one. */
    @Test
    fun aTestEventNeverRunsAKeyMap() {
        assertEquals(Handling.TestDropped, Relay.handle(TEST_PAYLOAD, test = true, now = 40_000, rules = rules))
    }

    /** The capture comes before the test is dropped, or the check could never hear its own test. */
    @Test
    fun theCheckHearsItsOwnTestEvent() {
        val heard = mutableListOf<Boolean>()
        Capture.arm(Capture.Wants.AnyEvent) { _, test -> heard += test }
        assertEquals(Handling.Captured, Relay.handle("030300240002000000A000BB01", test = true, now = 50_000, rules = rules))
        assertEquals(listOf(true), heard)
    }

    /** Learn takes presses only, so a late test event falls through and is dropped. */
    @Test
    fun learnLeavesATestEventToBeDropped() {
        Capture.arm(Capture.Wants.BandSignal) { _, _ -> }
        assertEquals(Handling.TestDropped, Relay.handle("030300250002000000A000BB01", test = true, now = 60_000, rules = rules))
    }

    /** The test event has a real bb01 press's bytes. A repeat judged on the payload alone would let either hide the other. */
    @Test
    fun aTestEventAndARealPressWithItsBytesDoNotHideEachOther() {
        assertEquals(Handling.TestDropped, Relay.handle(TEST_PAYLOAD, test = true, now = 80_000, rules = rules))
        assertEquals(Handling.Send(rules), Relay.handle(TEST_PAYLOAD, test = false, now = 80_100, rules = rules))
        assertEquals(Handling.Send(rules), Relay.handle(TEST_PAYLOAD, test = false, now = 90_000, rules = rules))
        assertEquals(Handling.TestDropped, Relay.handle(TEST_PAYLOAD, test = true, now = 90_100, rules = rules))
    }

    /** The repeat comes before the capture, so the check does not hear one press twice. */
    @Test
    fun aCaptureTakesAPressOnceThoughItArrivesTwice() {
        var taken = 0
        Capture.arm(Capture.Wants.AnyEvent) { _, _ -> taken++ }
        Relay.handle("030300260002000000A000BB01", test = false, now = 70_000, rules = rules)
        assertEquals(Handling.Duplicate, Relay.handle("030300260002000000A000BB01", test = false, now = 70_010, rules = rules))
        assertEquals(1, taken)
    }

    @Test
    fun theBandsOwnTrafficIsNeverCaptured() {
        Capture.arm(Capture.Wants.AnyEvent) { _, _ -> fail("traffic reached the check") }
        assertEquals(Handling.BandTraffic, Relay.handle("030B00410002000000" + "1B00" + "0505", test = false, now = 50_000, rules = rules))
    }

    @Test
    fun aVolumeKeyIsDroppedAndNeverCaptured() {
        Capture.arm(Capture.Wants.AnyEvent) { _, _ -> fail("a volume key reached the check") }
        assertEquals(Handling.BandTraffic, Relay.handle("030300440002000000" + "1B00" + "0505", test = false, now = 80_000, rules = rules))
    }
}
