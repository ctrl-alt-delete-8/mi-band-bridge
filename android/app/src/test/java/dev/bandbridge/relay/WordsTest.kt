package dev.bandbridge.relay

import java.time.ZoneId
import java.time.ZonedDateTime
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class WordsTest {
    /**
     * Setup's two notices under the required apps: the band needs a custom
     * band app, from the desktop app, whose page the notice opens; or,
     * without the desktop app, the sample band app, from the same page.
     */
    @Test
    fun setupSaysWhereTheBandAppComesFrom() {
        assertEquals(
            "The band also needs a custom band app. Install one with the desktop app from:",
            Words.DESKTOP_APP_NOTICE,
        )
        assertEquals(
            "Without the desktop app, download the sample band app, BandArrows, from the same page, and install it with Gadgetbridge.",
            Words.SAMPLE_APP_NOTICE,
        )
    }

    @Test
    fun saveSaysWhatIsMissing() {
        assertEquals("Add a signal, a name, and a key map id to save.", Words.missing("", "", ""))
        assertEquals("Add a signal to save.", Words.missing("", "Page down", "3f2a"))
        assertEquals("Add a name and a key map id to save.", Words.missing("bb01", " ", ""))
        assertEquals("Add a signal and a key map id to save.", Words.missing(" ", "Page down", ""))
        assertNull(Words.missing("bb01", "Page down", "3f2a"))
    }

    /** A missing field is named first. The signal rule comes up once all three are there (D11). */
    @Test
    fun aSignalThatIsNotASignalSaysSo() {
        assertEquals("Add a name to save.", Words.missing("zz", "", "3f2a"))
        assertEquals("A signal is hex that starts with bb, such as bb01.", Words.missing("zz", "Page down", "3f2a"))
    }

    @Test
    fun theLastPressIsATimeTodayAndADateBefore() {
        val utc = ZoneId.of("UTC")
        val now = ZonedDateTime.of(2026, 9, 26, 12, 0, 0, 0, utc).toInstant().toEpochMilli()
        val today = ZonedDateTime.of(2026, 9, 26, 9, 41, 8, 0, utc).toInstant().toEpochMilli()
        val before = ZonedDateTime.of(2026, 9, 25, 9, 41, 8, 0, utc).toInstant().toEpochMilli()
        assertEquals("Last band press arrived at 09:41:08.", Words.lastPress(today, now, utc))
        assertEquals("Last band press arrived on Sep 25 at 09:41.", Words.lastPress(before, now, utc))
        assertEquals("No band press has arrived yet.", Words.lastPress(null, now, utc))
    }

    /**
     * A press forty minutes ago, before midnight, arrived yesterday: today is
     * the date in the phone's zone, not the last 24 hours. The same two
     * moments fall on one day in UTC.
     */
    @Test
    fun todayIsTheDateInThePhonesZone() {
        val tokyo = ZoneId.of("Asia/Tokyo")
        val now = ZonedDateTime.of(2026, 9, 26, 0, 30, 0, 0, tokyo).toInstant().toEpochMilli()
        val press = ZonedDateTime.of(2026, 9, 25, 23, 50, 0, 0, tokyo).toInstant().toEpochMilli()
        assertEquals("Last band press arrived on Sep 25 at 23:50.", Words.lastPress(press, now, tokyo))
        assertEquals("Last band press arrived at 14:50:00.", Words.lastPress(press, now, ZoneId.of("UTC")))
    }

    /**
     * Frame 12: the press's label and the time it arrived, to the second, in
     * the phone's zone. The label goes into the line as it is given.
     */
    @Test
    fun theCheckSaysWhatArrivedAndWhen() {
        val at = ZonedDateTime.of(2026, 9, 26, 9, 42, 10, 0, ZoneId.of("UTC")).toInstant().toEpochMilli()
        assertEquals(
            "bb01 arrived at 09:42:10.",
            Words.arrived("bb01", at, ZoneId.of("UTC")),
        )
        assertEquals(
            "0303 arrived at 18:42:10.",
            Words.arrived("0303", at, ZoneId.of("Asia/Tokyo")),
        )
    }

    @Test
    fun theBackupLinesCountTheirRules() {
        assertEquals("Exported 3 rules to bandbridge-rules-2026-09-27.json.", Words.exported(3, "bandbridge-rules-2026-09-27.json"))
        assertEquals("Exported 1 rule to r.json.", Words.exported(1, "r.json"))
        assertEquals("Imported 2 rules from bandbridge-rules-2026-09-27.json.", Words.imported(2, "bandbridge-rules-2026-09-27.json"))
        assertEquals("Imported 0 rules from r.json.", Words.imported(0, "r.json"))
    }

    /**
     * Frame 17 starts the file's name on a line of its own, so a name is
     * never split at one of its hyphens. The name ends the line, so a name
     * that also appears earlier in it moves only where it ends the line.
     */
    @Test
    fun theFileNameStartsALineOfItsOwn() {
        val file = "bandbridge-rules-2026-09-27.json"
        assertEquals("Imported 2 rules from\nbandbridge-rules-2026-09-27.json.", Words.fileOnItsOwnLine(Words.imported(2, file), file))
        assertEquals("Exported 3 rules to\nrules.", Words.fileOnItsOwnLine(Words.exported(3, "rules"), "rules"))
        assertEquals(Words.NOT_A_RULES_FILE, Words.fileOnItsOwnLine(Words.NOT_A_RULES_FILE, "r.json"))
    }
}
