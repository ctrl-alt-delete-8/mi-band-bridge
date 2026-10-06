package dev.bandbridge.relay

import org.junit.Assert.assertEquals
import org.junit.Test

class GateTest {
    @Test
    fun theGateAcceptsEveryGadgetbridgeBuild() {
        val installed = setOf("nodomain.freeyourgadget.gadgetbridge.nightly", "io.github.sds100.keymapper")
        assertEquals(emptySet<RequiredApp>(), Gate.missing { it in installed })
    }

    @Test
    fun theGateNamesWhatIsMissing() {
        assertEquals(setOf(RequiredApp.KeyMapper), Gate.missing { it == "nodomain.freeyourgadget.gadgetbridge" })
        assertEquals(setOf(RequiredApp.Gadgetbridge, RequiredApp.KeyMapper), Gate.missing { false })
    }
}
