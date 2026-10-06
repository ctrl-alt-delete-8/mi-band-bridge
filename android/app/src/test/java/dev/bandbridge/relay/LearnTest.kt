package dev.bandbridge.relay

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * A round of Learn from the band against the real capture, as the receiver
 * offers it events: what ends a round, and that every end lets go of it.
 */
class LearnTest {
    private val ends = mutableListOf<Learn.End>()
    private val learn = Learn { ends += it }

    @After
    fun disarm() = Capture.disarm()

    /** The press fills in the signal and runs nothing: the capture takes it, then lets go. */
    @Test
    fun aPressEndsTheRoundWithItsSignal() {
        learn.start()
        assertTrue(Capture.offer("030300710002000000A000BB03", test = false))
        assertEquals(listOf<Learn.End>(Learn.End.Heard("bb03")), ends)
        assertFalse(learn.listening)
        assertFalse(Capture.armed)
    }

    @Test
    fun tenSecondsWithNoPressEndTheRound() {
        learn.start()
        repeat(LISTEN_SECONDS - 1) { learn.tick() }
        assertEquals(1, learn.secondsLeft)
        assertTrue(Capture.armed)
        assertEquals(emptyList<Learn.End>(), ends)
        learn.tick()
        assertEquals(listOf<Learn.End>(Learn.End.NoPress), ends)
        assertFalse(Capture.armed)
    }

    /**
     * Review Focus 2: Cancel, Back, a page over the editor and the app going
     * to the background all stop the round, and the next press is an
     * ordinary press again, with its key maps and its event row.
     */
    @Test
    fun aStoppedRoundTakesNothing() {
        learn.start()
        learn.stop()
        assertFalse(learn.listening)
        assertFalse(Capture.offer("030300720002000000A000BB02", test = false))
        assertEquals(emptyList<Learn.End>(), ends)
    }

    /** Other band traffic and the check's test event are not presses, and the round goes on. */
    @Test
    fun onlyABandAppPressIsLearned() {
        learn.start()
        assertFalse(Capture.offer("0303004F00020000001B00BB01", test = false))
        assertFalse(Capture.offer(TEST_PAYLOAD, test = true))
        assertTrue(learn.listening)
        assertEquals(emptyList<Learn.End>(), ends)
    }

    /** A second already on its way when a press ended the round, or a Cancel after it, changes nothing. */
    @Test
    fun aRoundEndsOnce() {
        learn.start()
        Capture.offer("030300730002000000A000BB01", test = false)
        learn.tick()
        learn.stop()
        assertEquals(listOf<Learn.End>(Learn.End.Heard("bb01")), ends)
    }

    /** Once its round is over, Learn never disarms a capture that the check has armed since. */
    @Test
    fun anEndedRoundLeavesAnotherCaptureAlone() {
        learn.start()
        learn.stop()
        Capture.arm(Capture.Wants.AnyEvent) { _, _ -> }
        learn.stop()
        learn.tick()
        assertTrue(Capture.armed)
    }

    /** Try again, after a round with no press, listens for the full time again. */
    @Test
    fun tryAgainStartsAFullRound() {
        learn.start()
        repeat(LISTEN_SECONDS) { learn.tick() }
        learn.start()
        assertTrue(learn.listening)
        assertEquals(LISTEN_SECONDS, learn.secondsLeft)
        assertTrue(Capture.armed)
    }
}
