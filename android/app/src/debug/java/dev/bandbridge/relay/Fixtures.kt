package dev.bandbridge.relay

import android.content.Context
import android.content.Intent
import android.util.Log
import dev.bandbridge.relay.ui.MainScreen
import dev.bandbridge.relay.ui.Navigator
import dev.bandbridge.relay.ui.Page
import dev.bandbridge.relay.ui.Screen
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZonedDateTime

/**
 * The design's example data, so each frame can be shot on the emulator
 * beside its picture (D18). Debug builds only: the release build has a
 * Fixtures that does nothing.
 *
 * A launch with `--es fixture <name>` wipes the relay's stored data and
 * seeds the fixture. A launch without the extra forgets any fixture, so the
 * app is never pretend in real use.
 *
 * - `working`: frame 02's three rules and five events. Both apps pretend to
 *   be installed.
 * - `empty`: no rules and no events (frame 04).
 * - `gate`: no rules, no events, and neither app installed (frame 01).
 * - `gate-keymapper`: as `gate`, but Gadgetbridge is installed, so the gate
 *   shows one installed app and one missing app.
 * - `long`: `working`, plus a rule with a long name and a long payload.
 * - `test-lost`: `working`, and the check's test event never leaves.
 * - `kit`: `working`, with the widget sheet in place of Main.
 * - `menu`: `working`, with the Rules menu open (frame 16).
 * - `imported`: frame 17, after an import: five rules, three events, and
 *   the line the import left under the Rules header.
 */
object Fixtures {
    /** The launch extra that names a fixture. */
    const val EXTRA = "fixture"

    const val WORKING = "working"
    const val EMPTY = "empty"
    const val GATE = "gate"
    const val GATE_KEYMAPPER = "gate-keymapper"
    const val LONG = "long"
    const val TEST_LOST = "test-lost"
    const val KIT = "kit"
    const val MENU = "menu"
    const val IMPORTED = "imported"

    /**
     * The file that frame 17 imported, named for the design's day, and the
     * line its import left. The line is not stored, so the fixture hands it
     * to Main as the page is built.
     */
    internal val IMPORTED_FILE: String = RulesFile.fileName(LocalDate.of(2026, 9, 27))
    internal val IMPORTED_LINE: String = Words.imported(2, IMPORTED_FILE)

    private const val PREFS = "fixture"
    private const val KEY_NAME = "name"
    private const val TAG = "Fixtures"

    /** What a fixture stores: rules, events newest first, the last arrival, and the apps it pretends are installed. */
    internal data class Seed(
        val rules: List<Rule>,
        val events: List<Event>,
        val lastArrival: Long?,
        val installed: Set<RequiredApp>,
    )

    private val BOTH_APPS: Set<RequiredApp> = RequiredApp.entries.toSet()

    fun apply(context: Context, intent: Intent) {
        val name = intent.getStringExtra(EXTRA)
        val store = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val seed = name?.let { seed(it, LocalDate.now(), ZoneId.systemDefault()) }
        if (seed == null) {
            if (name != null) {
                Log.w(TAG, "No fixture is named \"$name\", so none is used")
            }
            store.edit().clear().commit()
            return
        }
        // Written at once, not in the background: the window reads it next.
        store.edit().putString(KEY_NAME, name).commit()
        relayPrefs(context).edit().clear().commit()
        Rules.save(context, seed.rules)
        for (event in seed.events.asReversed()) {
            Events.add(context, event)
        }
        seed.lastArrival?.let { Events.markArrival(context, it) }
    }

    /**
     * Debug: the fixture's pretend install state, or null for the real one.
     *
     * The answer comes from the fixture's seed, one app at a time, so the
     * seed is the one record of what a fixture pretends.
     */
    fun pretendInstalled(context: Context, pkg: String): Boolean? {
        val name = stored(context) ?: return null
        val seed = seed(name, LocalDate.now(), ZoneId.systemDefault()) ?: return null
        return pretends(seed.installed, pkg)
    }

    /**
     * Whether [installed] holds the app that [pkg] is, or null when [pkg] is
     * none of the required apps. Any Gadgetbridge build counts as
     * Gadgetbridge, as it does for the gate. Pure, so it is tested without a
     * device.
     */
    internal fun pretends(installed: Set<RequiredApp>, pkg: String): Boolean? {
        val app = when (pkg) {
            in Gate.GADGETBRIDGE -> RequiredApp.Gadgetbridge
            Gate.KEY_MAPPER -> RequiredApp.KeyMapper
            else -> return null
        }
        return app in installed
    }

    /** Debug: true under fixture `test-lost`, so frame 15 can be shot. */
    fun dropTestEvent(context: Context): Boolean = stored(context) == TEST_LOST

    /**
     * Debug: a page in place of the one Main would be: the widget sheet
     * under fixture `kit`, and Main as frames 16 and 17 show it under `menu`
     * and `imported`. A launch cannot tap, so the fixture opens the menu, or
     * gives Main the line an import leaves, as the page is built.
     */
    fun page(context: Context, screen: Screen, navigator: Navigator): Page? {
        if (screen != Screen.Main) {
            return null
        }
        return when (stored(context)) {
            KIT -> KitPage(context, navigator)
            MENU -> MainScreen(context, navigator).apply { openMenu() }
            IMPORTED -> MainScreen(context, navigator).apply { showResult(IMPORTED_LINE, IMPORTED_FILE) }
            else -> null
        }
    }

    /**
     * The fixture named [name], with its times on [day] in [zone], or null
     * when there is no such fixture. Pure, so the data is tested without a
     * device.
     */
    internal fun seed(name: String, day: LocalDate, zone: ZoneId): Seed? {
        fun at(hour: Int, minute: Int, second: Int): Long =
            ZonedDateTime.of(day, LocalTime.of(hour, minute, second), zone).toInstant().toEpochMilli()

        val working = Seed(
            rules = listOf(
                Rule("bb01", "Page down", "3f2a9c1e-7b4d-4e0a-9c61-2d8e5f0a1b3c"),
                Rule("bb01", "Next slide", "8d7b2e4a-1c69-4f03-a512-9e0b6c3d8f21"),
                Rule("bb02", "Page up", "6a4c0d92-8f31-4b7e-932a-5d1e0c8b6f47"),
            ),
            events = listOf(
                Event(at(9, 41, 8), press("51", "BB01"), listOf("Page down", "Next slide")),
                Event(at(9, 40, 52), press("50", "BB02"), listOf("Page up")),
                Event(at(9, 40, 37), press("4F", "BB03"), emptyList()),
                // Frame 02 prints "Not a band app signal" here, under the
                // chip 0303. The relay now records band app presses only
                // (A1), so this row is one it can make: a bb02 press (A4).
                Event(at(9, 40, 21), press("4E", "BB02"), listOf("Page up")),
                Event(at(9, 40, 5), press("4D", "BB02"), listOf("Page up")),
            ),
            lastArrival = at(9, 41, 8),
            installed = BOTH_APPS,
        )
        return when (name) {
            WORKING, TEST_LOST, KIT, MENU -> working
            // Frame 17: the import added two rules after frame 02's three.
            // Its events are frame 02's first three, and bb03's still says
            // no rule, since an event keeps what happened when it arrived.
            IMPORTED -> working.copy(
                rules = working.rules + listOf(
                    Rule("bb03", "Play or pause", "c41e8a07-5d2b-4f9c-b6a3-7e0d19f2c584"),
                    Rule("bb04", "Previous slide", "0b9d3f6e-2a71-48c5-9e14-d5a8c7b30e62"),
                ),
                events = working.events.take(3),
            )
            EMPTY -> Seed(emptyList(), emptyList(), lastArrival = null, installed = BOTH_APPS)
            GATE -> Seed(emptyList(), emptyList(), lastArrival = null, installed = emptySet())
            GATE_KEYMAPPER -> Seed(
                emptyList(), emptyList(), lastArrival = null, installed = setOf(RequiredApp.Gadgetbridge),
            )
            LONG -> {
                val traffic = Event(at(9, 41, 21), LONG_PAYLOAD, emptyList())
                working.copy(
                    rules = working.rules + Rule(
                        "bb04",
                        "Scroll down in the reader app, then wait for the page",
                        "5b9e1f7a-2c84-4d36-b0e5-7a1c9f3d2e68",
                    ),
                    events = Events.added(working.events, traffic),
                    lastArrival = traffic.at,
                )
            }
            else -> null
        }
    }

    private fun stored(context: Context): String? =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY_NAME, null)

    /** A band app press on endpoint 0x00a0, as Gadgetbridge sends it: [handle] and a two byte [signal], in hex. */
    private fun press(handle: String, signal: String): String = "030300${handle}0002000000A000$signal"

    /**
     * A 60 byte chunk on endpoint 0x001b, the music endpoint: the band's own
     * traffic, which the relay drops. Stored here as 2.1.0 stored such a
     * row, it also shows that an old row still draws. Its 120 hex characters
     * are more than one line can hold on any phone.
     */
    private val LONG_PAYLOAD: String =
        "0303005200" + "31000000" + "1B00" + (0 until 49).joinToString("") { it.toString(16).padStart(2, '0').uppercase() }
}
