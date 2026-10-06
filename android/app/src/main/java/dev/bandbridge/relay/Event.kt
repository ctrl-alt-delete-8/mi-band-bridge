package dev.bandbridge.relay

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

/**
 * One band event as the main page lists it: when it arrived, its raw payload,
 * and the names of the rules it triggered.
 *
 * The names are kept, not the rules, so a row keeps saying what happened at
 * the time even after its rule is renamed or removed.
 */
data class Event(val at: Long, val payload: String, val triggered: List<String>) {
    /**
     * The band app signal, or null when the event has none: a row that an
     * earlier version stored for the band's own traffic, or for a music key,
     * which 2.2.0 recorded (A1).
     */
    val signal: String? get() = Signals.signalOf(payload)

    /** What the row's chip shows. */
    val label: String get() = Signals.labelOf(payload)

    fun outcome(): Outcome = when {
        triggered.isNotEmpty() -> Outcome.Triggered(triggered)
        signal != null -> Outcome.NoRule
        else -> Outcome.NotBandSignal
    }
}

/** What an event did. Its row says it in words, and offers what fits. */
sealed interface Outcome {
    /** The event sent the key maps of the rules named here. */
    data class Triggered(val names: List<String>) : Outcome

    /** A press, but no rule has its signal. The row offers to add one. */
    data object NoRule : Outcome

    /**
     * A row that an earlier version stored for a payload with no signal: a
     * music key that 2.2.0 recorded, such as its vol+, or the band's own
     * traffic that 2.1.0 recorded. The relay records neither now (A1). The
     * row shows its raw bytes.
     */
    data object NotBandSignal : Outcome
}

/** An event row's words, as the design writes them. */
fun Outcome.words(): String = when (this) {
    is Outcome.Triggered -> "Triggered " + names.joinToString(", ")
    Outcome.NoRule -> "No rule for this signal"
    Outcome.NotBandSignal -> "Not a band app signal"
}

/**
 * The last few band events, newest first, and when a press last arrived.
 *
 * Only [KEEP] are kept. The design shows five on the phone and the tablet, and
 * five keep the rules on the first screen of a phone.
 */
object Events {
    const val KEEP = 5

    private const val KEY_EVENTS = "events_v2"
    private const val KEY_LAST_ARRIVAL = "last_arrival_at"

    fun encode(events: List<Event>): String {
        val array = JSONArray()
        for (event in events) {
            array.put(
                JSONObject()
                    .put("at", event.at)
                    .put("payload", event.payload)
                    .put("triggered", JSONArray(event.triggered))
            )
        }
        return array.toString()
    }

    /** Like the rules, an unreadable store is no events, and a broken event is skipped. */
    fun decode(json: String): List<Event> = readArray(json) { item ->
        val at = (item.opt("at") as? Number)?.toLong()
        val payload = item.text("payload")
        val names = item.optJSONArray("triggered")
        if (at == null || payload == null || names == null) {
            null
        } else {
            Event(at, payload, (0 until names.length()).mapNotNull { names.opt(it) as? String })
        }
    }

    /** Newest first, at most KEEP. Pure, so the rule is tested without a device. */
    fun added(events: List<Event>, event: Event): List<Event> =
        (listOf(event) + events).take(KEEP)

    fun all(context: Context): List<Event> =
        relayPrefs(context).getString(KEY_EVENTS, null)?.let { decode(it) }.orEmpty()

    /** Synchronized, because it reads the list and then writes it back. */
    @Synchronized
    fun add(context: Context, event: Event) {
        relayPrefs(context).edit()
            .putString(KEY_EVENTS, encode(added(all(context), event)))
            .apply()
    }

    /** Empties the list. The last arrival stays, because Setup reads it. */
    @Synchronized
    fun clear(context: Context) {
        relayPrefs(context).edit().remove(KEY_EVENTS).apply()
    }

    /** When a band press last arrived (C5). Clear does not wipe it (D12). */
    fun lastArrival(context: Context): Long? {
        val prefs = relayPrefs(context)
        return if (prefs.contains(KEY_LAST_ARRIVAL)) prefs.getLong(KEY_LAST_ARRIVAL, 0L) else null
    }

    /**
     * Stores when a band press arrived.
     *
     * The receiver calls this for every press, a captured one included, and
     * never for a test event or the band's own traffic, its own buttons
     * included (C5, A1). A test proves nothing about the band, traffic is no
     * press in a band app, and Setup's last press line must not report a
     * press that never came.
     */
    fun markArrival(context: Context, at: Long) {
        relayPrefs(context).edit().putLong(KEY_LAST_ARRIVAL, at).apply()
    }
}
