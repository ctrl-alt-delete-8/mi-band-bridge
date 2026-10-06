package dev.bandbridge.relay

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class EventsTest {
    @Test
    fun anEventSaysWhatItTriggered() {
        assertEquals("Triggered Page down, Next slide",
            Event(0, "0303004F0002000000A000BB01", listOf("Page down", "Next slide")).outcome().words())
        assertEquals("No rule for this signal", Event(0, "0303004F0002000000A000BB03", emptyList()).outcome().words())
        assertEquals("Not a band app signal", Event(0, "0303004F00020000001B00BB01", emptyList()).outcome().words())
    }

    @Test
    fun eventsKeepTheNewestFiveNewestFirst() {
        val kept = (1..8).fold(emptyList<Event>()) { list, n ->
            Events.added(list, Event(n.toLong(), "0303004F0002000000A000BB01", emptyList()))
        }
        assertEquals(listOf(8L, 7L, 6L, 5L, 4L), kept.map { it.at })
    }

    /**
     * A row of the band's own traffic that 2.0.0 or 2.1.0 stored still draws
     * as it did, and presses push it out as they push out any row.
     */
    @Test
    fun aTrafficRowStoredBeforeStillDrawsAndRollsOff() {
        val old = Event(1, "030B00410002000000" + "1B00" + "0505", emptyList())
        assertEquals("030B Not a band app signal", "${old.label} ${old.outcome().words()}")
        val kept = (2..6).fold(listOf(old)) { list, n ->
            Events.added(list, Event(n.toLong(), "0303004F0002000000A000BB01", emptyList()))
        }
        assertEquals(listOf(6L, 5L, 4L, 3L, 2L), kept.map { it.at })
    }

    /**
     * Review Focus 2: a row that 2.2.0 stored for a music key, such as its
     * vol+, still draws after the update, as a row of other traffic does:
     * its first two bytes and "Not a band app signal". It has no signal, so
     * it offers no rule.
     */
    @Test
    fun aMusicKeyRowStoredBy220StillDraws() {
        val old = Event(1, "030300440002000000" + "1B00" + "0505", emptyList())
        assertEquals("0303 Not a band app signal", "${old.label} ${old.outcome().words()}")
        assertNull(old.signal)
    }

    @Test
    fun eventsRoundTrip() {
        val events = listOf(Event(1_727_000_000_000, "0303004F0002000000A000BB01", listOf("Page down")))
        assertEquals(events, Events.decode(Events.encode(events)))
        assertEquals(emptyList<Event>(), Events.decode("{"))
    }
}
