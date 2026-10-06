package dev.bandbridge.relay

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CaptureTest {
    @After
    fun disarm() = Capture.disarm()

    @Test
    fun learnTakesABandSignalAndNothingElse() {
        val took = mutableListOf<String>()
        Capture.arm(Capture.Wants.BandSignal) { payload, _ -> took += payload }
        assertFalse(Capture.offer("0303004F00020000001B00BB01", test = false))
        assertFalse(Capture.offer("0303004F0002000000A000BB01", test = true))
        assertTrue(Capture.offer("0303004F0002000000A000BB02", test = false))
        assertEquals(listOf("0303004F0002000000A000BB02"), took)
    }

    @Test
    fun learnDoesNotTakeAVolumeKey() {
        Capture.arm(Capture.Wants.BandSignal) { _, _ -> }
        assertFalse(Capture.offer("030300120002000000" + "1B00" + "0505", test = false))
    }

    @Test
    fun checkTakesEveryEventAndKnowsATest() {
        val tests = mutableListOf<Boolean>()
        Capture.arm(Capture.Wants.AnyEvent) { _, test -> tests += test }
        assertTrue(Capture.offer("0303004F00020000001B00BB01", test = false))
        assertTrue(Capture.offer(TEST_PAYLOAD, test = true))
        assertEquals(listOf(false, true), tests)
    }

    @Test
    fun aDisarmedCaptureTakesNothing() {
        Capture.arm(Capture.Wants.AnyEvent) { _, _ -> }
        Capture.disarm()
        assertFalse(Capture.armed)
        assertFalse(Capture.offer("0303004F0002000000A000BB01", test = false))
    }
}
