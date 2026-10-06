package dev.bandbridge.relay.ui

import org.junit.Assert.assertEquals
import org.junit.Test

/** Where an open menu goes against its ⋮ button. The numbers are frame 16's, in dp. */
class MenuSpotTest {
    /** Frame 16: ⋮ spans 348 to 396 across and 419 to 467 down, and the 220 by 105 menu sits 4 under it. */
    @Test
    fun theMenuOpensUnderItsButtonAsFrame16DrawsIt() {
        assertEquals(
            MenuSpot(left = 176, top = 471),
            MenuSpot.of(anchorRight = 396, anchorTop = 419, anchorBottom = 467, width = 220, height = 105, gap = 4, layerHeight = 891),
        )
    }

    @Test
    fun exactlyEnoughRoomUnderTheButtonIsEnough() {
        assertEquals(MenuSpot(176, 471), MenuSpot.of(396, 419, 467, 220, 105, 4, layerHeight = 576))
    }

    /** Low on a short screen, the menu opens above its button, so that Import rules is never cut off. */
    @Test
    fun withNoRoomUnderTheButtonItOpensAboveIt() {
        assertEquals(MenuSpot(176, 310), MenuSpot.of(396, 419, 467, 220, 105, 4, layerHeight = 575))
    }

    /** A window too short for the menu on either side of its button still shows all of it, as low as it fits. */
    @Test
    fun withRoomOnNeitherSideItStaysWholeInTheWindow() {
        assertEquals(MenuSpot(176, 15), MenuSpot.of(396, 60, 108, 220, 105, 4, layerHeight = 120))
        assertEquals(MenuSpot(176, 0), MenuSpot.of(396, 60, 108, 220, 105, 4, layerHeight = 90))
    }

    @Test
    fun itNeverRunsOffTheLeftEdge() {
        assertEquals(MenuSpot(0, 471), MenuSpot.of(200, 419, 467, 220, 105, 4, 891))
    }
}
