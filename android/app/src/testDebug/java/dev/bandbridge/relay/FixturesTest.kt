package dev.bandbridge.relay

import java.time.LocalDate
import java.time.ZoneId
import java.time.ZonedDateTime
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The debug fixtures hold the design's example data, read through the same
 * code the pages use, so a shot of a fixture can be put beside its frame.
 * These tests live in the debug unit tests, because the release build has
 * no fixtures.
 */
class FixturesTest {
    private val day = LocalDate.of(2026, 9, 26)
    private val utc = ZoneId.of("UTC")

    private fun seed(name: String) = Fixtures.seed(name, day, utc)!!

    private fun at(hour: Int, minute: Int, second: Int): Long =
        ZonedDateTime.of(2026, 9, 26, hour, minute, second, 0, utc).toInstant().toEpochMilli()

    /**
     * Each row as frame 02 prints it: chip, then outcome. The fourth row is a
     * bb02 press, not the frame's "Not a band app signal", by the user's
     * decision (A4).
     */
    @Test
    fun workingReadsAsFrame02() {
        val working = seed(Fixtures.WORKING)
        assertEquals(
            listOf(
                "bb01 Triggered Page down, Next slide",
                "bb02 Triggered Page up",
                "bb03 No rule for this signal",
                "bb02 Triggered Page up",
                "bb02 Triggered Page up",
            ),
            working.events.map { "${it.label} ${it.outcome().words()}" },
        )
        assertEquals(
            listOf(at(9, 41, 8), at(9, 40, 52), at(9, 40, 37), at(9, 40, 21), at(9, 40, 5)),
            working.events.map { it.at },
        )
        assertEquals(listOf("Page down", "Next slide", "Page up"), working.rules.map { it.name })
        assertEquals(at(9, 41, 8), working.lastArrival)
        assertEquals(RequiredApp.entries.toSet(), working.installed)
    }

    /**
     * The relay records band app presses only (A1), so each row of frame 02
     * is one, as the relay would record it: the fourth as well, a bb02 press
     * where 2.2.0 had a volume up (A4).
     */
    @Test
    fun everyRowOfWorkingIsABandAppPress() {
        for (event in seed(Fixtures.WORKING).events) {
            assertTrue(event.payload, Signals.pressOf(event.payload) is Press.Signal)
        }
    }

    /** Every signal in the fixtures follows the rule the editor and the Mac apply. */
    @Test
    fun everyFixtureSignalIsASignal() {
        for (rule in seed(Fixtures.LONG).rules + seed(Fixtures.IMPORTED).rules) {
            assertEquals(rule.signal, Signals.normalize(rule.signal))
        }
    }

    /**
     * Frame 17: frame 02's three rules and the two the import added after
     * them, and frame 02's first three events, as the frame prints them.
     */
    @Test
    fun importedReadsAsFrame17() {
        val imported = seed(Fixtures.IMPORTED)
        assertEquals(seed(Fixtures.WORKING).rules, imported.rules.take(3))
        assertEquals(
            listOf(
                Rule("bb03", "Play or pause", "c41e8a07-5d2b-4f9c-b6a3-7e0d19f2c584"),
                Rule("bb04", "Previous slide", "0b9d3f6e-2a71-48c5-9e14-d5a8c7b30e62"),
            ),
            imported.rules.drop(3),
        )
        assertEquals(
            listOf("bb01 Triggered Page down, Next slide", "bb02 Triggered Page up", "bb03 No rule for this signal"),
            imported.events.map { "${it.label} ${it.outcome().words()}" },
        )
        assertEquals(at(9, 41, 8), imported.lastArrival)
        assertEquals(RequiredApp.entries.toSet(), imported.installed)
    }

    /** Frame 17's line says the file added 2 rules to frame 02's three, and the real merge agrees. */
    @Test
    fun importedIsWhatAnImportAddsToWorking() {
        val working = seed(Fixtures.WORKING).rules
        val imported = seed(Fixtures.IMPORTED).rules
        assertEquals(imported, RulesFile.merged(working, imported))
        assertEquals(2, imported.size - working.size)
        assertEquals("Imported 2 rules from mi-band-bridge-rules-2026-09-27.json.", Fixtures.IMPORTED_LINE)
    }

    /** Review Focus 4: the values that must wrap on a 412 dp phone. */
    @Test
    fun longHasALongNameAnIdAndA120CharacterPayload() {
        val long = seed(Fixtures.LONG)
        val rule = long.rules.last()
        assertEquals("Scroll down in the reader app, then wait for the page", rule.name)
        assertEquals(36, rule.keyMapUid.length)
        val traffic = long.events.first()
        assertEquals(120, traffic.payload.length)
        assertNull(traffic.signal)
        assertEquals(Events.KEEP, long.events.size)
        assertEquals(traffic.at, long.lastArrival)
    }

    @Test
    fun emptyAndTheGatesHaveNothingAndDifferOnlyInTheApps() {
        val empty = seed(Fixtures.EMPTY)
        val gate = seed(Fixtures.GATE)
        val keyMapperGate = seed(Fixtures.GATE_KEYMAPPER)
        for (nothing in listOf(empty, gate, keyMapperGate)) {
            assertTrue(nothing.rules.isEmpty())
            assertTrue(nothing.events.isEmpty())
            assertNull(nothing.lastArrival)
        }
        assertEquals(RequiredApp.entries.toSet(), empty.installed)
        assertEquals(emptySet<RequiredApp>(), gate.installed)
        assertEquals(setOf(RequiredApp.Gadgetbridge), keyMapperGate.installed)
    }

    /** Through the real gate: what each fixture pretends is what the gate finds missing. */
    @Test
    fun theGateFindsMissingWhatTheFixturePretends() {
        fun missing(name: String) = Gate.missing { Fixtures.pretends(seed(name).installed, it) ?: false }
        assertEquals(emptySet<RequiredApp>(), missing(Fixtures.WORKING))
        assertEquals(setOf(RequiredApp.Gadgetbridge, RequiredApp.KeyMapper), missing(Fixtures.GATE))
        assertEquals(setOf(RequiredApp.KeyMapper), missing(Fixtures.GATE_KEYMAPPER))
    }

    /** Any Gadgetbridge build counts, and a package that is no required app gets no pretend answer. */
    @Test
    fun aFixtureAnswersForEachRequiredAppAndNothingElse() {
        val gadgetbridgeOnly = setOf(RequiredApp.Gadgetbridge)
        for (build in Gate.GADGETBRIDGE) {
            assertEquals(true, Fixtures.pretends(gadgetbridgeOnly, build))
        }
        assertEquals(false, Fixtures.pretends(gadgetbridgeOnly, Gate.KEY_MAPPER))
        assertNull(Fixtures.pretends(gadgetbridgeOnly, "com.example.other"))
    }

    @Test
    fun kitAndTestLostStartFromWorking() {
        assertEquals(seed(Fixtures.WORKING), seed(Fixtures.KIT))
        assertEquals(seed(Fixtures.WORKING), seed(Fixtures.TEST_LOST))
    }

    /** Frame 16 is frame 02 with the menu open. */
    @Test
    fun menuStartsFromWorking() {
        assertEquals(seed(Fixtures.WORKING), seed(Fixtures.MENU))
    }

    @Test
    fun anUnknownNameIsNoFixture() {
        assertNull(Fixtures.seed("frame02", day, utc))
    }
}
