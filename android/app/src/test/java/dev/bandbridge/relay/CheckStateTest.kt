package dev.bandbridge.relay

import org.junit.Assert.assertEquals
import org.junit.Test

/** The connection check's flow, without views: what each event, timer and button leads to. */
class CheckStateTest {
    @Test
    fun aPressDuringTheWatchMeansItWorks() {
        assertEquals(CheckState.Arrived("bb01", 5), next(CheckState.Watching, Happening.Heard("0303004F0002000000A000BB01", 5, test = false)))
    }

    @Test
    fun noPressThenATestThatArrivesBlamesGadgetbridge() {
        val noPress = next(CheckState.Watching, Happening.TimedOut)
        assertEquals(CheckState.NoPress, noPress)
        val sent = next(noPress, Happening.SendTest)
        assertEquals(CheckState.TestSent, sent)
        assertEquals(CheckState.FaultInGadgetbridge, next(sent, Happening.Heard(TEST_PAYLOAD, 9, test = true)))
        assertEquals(CheckState.FaultInThisApp, next(sent, Happening.TimedOut))
    }

    /** A real press while the test is out still proves Gadgetbridge works. */
    @Test
    fun aRealPressWhileTheTestIsOutMeansItWorks() {
        assertEquals(CheckState.Arrived("bb02", 7), next(CheckState.TestSent, Happening.Heard("030300330002000000A000BB02", 7, test = false)))
    }

    /** A test from an earlier round must not pass for a band press. */
    @Test
    fun aLateTestDuringTheWatchIsIgnored() {
        assertEquals(CheckState.Watching, next(CheckState.Watching, Happening.Heard(TEST_PAYLOAD, 3, test = true)))
    }

    /**
     * The button looks active while the test is out, so it works: the check
     * gives the test up and watches again. A test that arrives after that
     * lands in the watch, which ignores it.
     */
    @Test
    fun tryingTheBandAgainWhileTheTestIsOutWatchesAgain() {
        val watching = next(CheckState.TestSent, Happening.TryBandAgain)
        assertEquals(CheckState.Watching, watching)
        assertEquals(CheckState.Watching, next(watching, Happening.Heard(TEST_PAYLOAD, 4, test = true)))
    }

    @Test
    fun eachResultLeadsWhereItsButtonsSay() {
        assertEquals(CheckState.Watching, next(CheckState.NoPress, Happening.TryBandAgain))
        assertEquals(CheckState.Watching, next(CheckState.FaultInGadgetbridge, Happening.TryBandAgain))
        assertEquals(CheckState.TestSent, next(CheckState.FaultInThisApp, Happening.SendTest))
        assertEquals(CheckState.FaultInThisApp, next(CheckState.FaultInThisApp, Happening.TimedOut))
    }

    /**
     * Every state against every happening: the moves the tests above name,
     * and no other. Any other pair leaves the state as it is, so a timer
     * that fires late, a second event, or a button of a state no longer
     * shown changes nothing.
     */
    @Test
    fun anyOtherPairLeavesTheStateAsItIs() {
        val press = Happening.Heard("030300340002000000A000BB01", 11, test = false)
        val test = Happening.Heard(TEST_PAYLOAD, 12, test = true)
        val moves = mapOf<Pair<CheckState, Happening>, CheckState>(
            (CheckState.Watching to press) to CheckState.Arrived("bb01", 11),
            (CheckState.Watching to Happening.TimedOut) to CheckState.NoPress,
            (CheckState.NoPress to Happening.SendTest) to CheckState.TestSent,
            (CheckState.NoPress to Happening.TryBandAgain) to CheckState.Watching,
            (CheckState.TestSent to press) to CheckState.Arrived("bb01", 11),
            (CheckState.TestSent to test) to CheckState.FaultInGadgetbridge,
            (CheckState.TestSent to Happening.TimedOut) to CheckState.FaultInThisApp,
            (CheckState.TestSent to Happening.TryBandAgain) to CheckState.Watching,
            (CheckState.FaultInGadgetbridge to Happening.TryBandAgain) to CheckState.Watching,
            (CheckState.FaultInThisApp to Happening.SendTest) to CheckState.TestSent,
        )
        for (state in ALL_STATES) {
            for (happened in listOf(press, test, Happening.TimedOut, Happening.SendTest, Happening.TryBandAgain)) {
                assertEquals("$state, $happened", moves[state to happened] ?: state, next(state, happened))
            }
        }
    }

    /**
     * D9: the watch and the wait for the test hold the capture, and every
     * result lets go of it, so a press after a result is an ordinary press
     * again, with its key maps and its event row.
     */
    @Test
    fun theCheckListensOnlyWhileItWatchesOrWaitsForItsTest() {
        assertEquals(listOf(true, false, false, true, false, false), ALL_STATES.map { it.listening })
    }

    private companion object {
        val ALL_STATES: List<CheckState> = listOf(
            CheckState.Watching,
            CheckState.Arrived("bb01", 5),
            CheckState.NoPress,
            CheckState.TestSent,
            CheckState.FaultInGadgetbridge,
            CheckState.FaultInThisApp,
        )
    }
}
