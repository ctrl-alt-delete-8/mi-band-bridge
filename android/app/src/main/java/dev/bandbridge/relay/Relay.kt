package dev.bandbridge.relay

/** What one delivered broadcast does. [BandReceiver] carries it out. */
sealed interface Handling {
    /** The second delivery of an event already handled. Nothing happens. */
    data object Duplicate : Handling

    /**
     * The band's own traffic with Gadgetbridge, its own buttons included,
     * which is no press (A1). Nothing happens, and nothing shows that it
     * came (C2).
     */
    data object BandTraffic : Handling

    /** Learn or the check took the event. No key map runs, and no row is added. */
    data object Captured : Handling

    /** A test event that no check took. Nothing happens. */
    data object TestDropped : Handling

    /**
     * A band app press: send each rule's key map, then add an event row. A
     * press that no rule names only adds its row.
     */
    data class Send(val matched: List<Rule>) : Handling
}

/**
 * The relay's one decision, kept apart from Android so it can be tested
 * without a device.
 */
object Relay {
    /**
     * In this order: a repeat is a Duplicate, the band's own traffic is
     * dropped, a wanted event is Captured, a test is dropped, the rest is
     * sent.
     *
     * The order carries the rules. The repeat comes first, so the second
     * delivery of a press never reaches a capture or Key Mapper. The band's
     * own traffic comes next, so no capture ever sees it: an encrypted
     * chunk or a volume key during the check's watch would otherwise read
     * "It works." (C2, A1). The capture comes before the test, so the check
     * hears its own test event. The test comes before the send, so a test
     * event never runs a key map, even one that arrives after the check has
     * stopped.
     *
     * A test event is a repeat only of a test event. It has a real press's
     * bytes, so a repeat judged on the payload alone would let either one
     * hide the other for a moment.
     */
    fun handle(payload: String, test: Boolean, now: Long, rules: List<Rule>): Handling {
        val sighting = if (test) "test $payload" else payload
        return when {
            !Signals.isFirstSighting(sighting, now) -> Handling.Duplicate
            Signals.pressOf(payload) == null -> Handling.BandTraffic
            Capture.offer(payload, test) -> Handling.Captured
            test -> Handling.TestDropped
            else -> Handling.Send(Signals.matchAll(payload, rules))
        }
    }
}
