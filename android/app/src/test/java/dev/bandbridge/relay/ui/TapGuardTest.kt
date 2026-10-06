package dev.bandbridge.relay.ui

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The quiet time after the screen changes, without views: which touches go
 * through. Times are MotionEvent event times, in milliseconds.
 */
class TapGuardTest {
    private val guard = TapGuard()

    /** The review's case: "Add a rule", then a second tap 200 ms later that would land on Learn. */
    @Test
    fun aSecondTapSoonAfterTheScreenChangesIsIgnored() {
        guard.changed(at = 10_000)
        assertFalse(guard.lets(down = true, at = 10_200))
        assertFalse(guard.lets(down = false, at = 10_260))
    }

    @Test
    fun aTapHalfASecondAfterTheChangeGoesThrough() {
        guard.changed(at = 10_000)
        assertFalse(guard.lets(down = true, at = 10_499))
        assertTrue(guard.lets(down = true, at = 10_500))
        assertTrue(guard.lets(down = false, at = 10_560))
    }

    /**
     * On a slow panel a finger can land while the new screen is still being
     * built, and its touch waits in the queue. It was aimed at the old screen.
     */
    @Test
    fun aTapThatLandedBeforeTheChangeWasHandledIsIgnored() {
        guard.changed(at = 10_000)
        assertFalse(guard.lets(down = true, at = 9_900))
    }

    /** A drag that starts in the quiet time is ignored to its end, even the part after it. */
    @Test
    fun aGestureIsJudgedByItsFirstTouch() {
        guard.changed(at = 10_000)
        assertFalse(guard.lets(down = true, at = 10_100))
        assertFalse(guard.lets(down = false, at = 10_700))
        assertFalse(guard.lets(down = false, at = 10_800))
        assertTrue(guard.lets(down = true, at = 10_900))
    }

    /** The tap that changed the screen, or a drag under way, is not cut off halfway. */
    @Test
    fun aGestureUnderWayWhenTheScreenChangesGoesOn() {
        assertTrue(guard.lets(down = true, at = 9_000))
        guard.changed(at = 9_100)
        assertTrue(guard.lets(down = false, at = 9_150))
    }

    /** A dialog that opens right after a page changes gets its own half second. */
    @Test
    fun eachChangeStartsTheQuietTimeAgain() {
        guard.changed(at = 10_000)
        guard.changed(at = 10_400)
        assertFalse(guard.lets(down = true, at = 10_700))
        assertTrue(guard.lets(down = true, at = 10_900))
    }

    @Test
    fun beforeAnyChangeEveryTapGoesThrough() {
        assertTrue(guard.lets(down = true, at = 1))
    }
}
