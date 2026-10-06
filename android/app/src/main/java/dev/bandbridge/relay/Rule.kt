package dev.bandbridge.relay

import android.content.Context
import android.content.SharedPreferences
import org.json.JSONArray
import org.json.JSONException
import org.json.JSONObject

/**
 * One rule: when the band sends [signal], Key Mapper runs the key map
 * [keyMapUid].
 *
 * [signal] is stored normalized, lower case hex such as `bb01`, so the Mac and
 * this app name a signal the same way. [name] is for the person only: the rule
 * list shows it, and an event row says which rules it triggered by it. Several
 * rules may share a signal, and each of them is sent.
 */
data class Rule(val signal: String, val name: String, val keyMapUid: String)

/**
 * The stored rules, in the order the person put them.
 *
 * Version 1.0.0 kept its rules as `signal=uid` lines under another key. The
 * first load after an update turns them into named rules, once, because the
 * band would stop working the moment the app updated if they were lost.
 */
object Rules {
    private const val KEY_RULES = "rules_v2"

    /** Where 1.0.0 kept its rules. */
    private const val KEY_OLD_RULES = "rules"

    /** Everything 1.0.0 stored: its rules, the last payload and its time, and its log. */
    private val OLD_KEYS = listOf(KEY_OLD_RULES, "last_payload", "last_at", "log")

    fun encode(rules: List<Rule>): String {
        val array = JSONArray()
        for (rule in rules) {
            array.put(
                JSONObject()
                    .put("signal", rule.signal)
                    .put("name", rule.name)
                    .put("keyMapUid", rule.keyMapUid)
            )
        }
        return array.toString()
    }

    /**
     * An unreadable store is no rules, never a crash.
     *
     * A crash here would come back on every press and every start, and hide
     * the fault behind it. A rule missing a field is skipped for the same
     * reason.
     */
    fun decode(json: String): List<Rule> = readArray(json) { item ->
        val signal = item.text("signal")
        val name = item.text("name")
        val keyMapUid = item.text("keyMapUid")
        if (signal == null || name == null || keyMapUid == null) {
            null
        } else {
            Rule(signal, name, keyMapUid)
        }
    }

    /**
     * The 1.0.0 `signal=uid` lines as named rules (D5).
     *
     * Each rule is named after its signal, such as "bb01". A line with no key
     * map id, or no signal, fired nothing in 1.0.0, so it is dropped. A signal
     * that does not follow the signal rule is kept as typed, trimmed and in
     * lower case, so the person can see it and fix it.
     */
    fun migrate(oldLines: String): List<Rule> = oldLines.lines().mapNotNull { line ->
        val at = line.indexOf('=')
        if (at < 0) {
            return@mapNotNull null
        }
        val typed = line.substring(0, at).trim()
        val keyMapUid = line.substring(at + 1).trim()
        if (typed.isEmpty() || keyMapUid.isEmpty()) {
            return@mapNotNull null
        }
        val signal = Signals.normalize(typed) ?: typed.lowercase()
        Rule(signal, signal, keyMapUid)
    }

    /**
     * The stored rules, after the one-time move from 1.0.0 (D5).
     *
     * When there is no `rules_v2` yet, the old lines are converted and saved
     * under it, and every 1.0.0 key is removed, in one edit. From then on
     * `rules_v2` exists, so the move never runs again. Synchronized, because
     * the move reads and then writes.
     */
    @Synchronized
    fun load(context: Context): List<Rule> {
        val prefs = relayPrefs(context)
        prefs.getString(KEY_RULES, null)?.let { return decode(it) }
        val rules = migrate(prefs.getString(KEY_OLD_RULES, null).orEmpty())
        val edit = prefs.edit().putString(KEY_RULES, encode(rules))
        for (key in OLD_KEYS) {
            edit.remove(key)
        }
        edit.apply()
        return rules
    }

    /**
     * The rules after the editor saves [rule]. A new rule, with no [was],
     * goes last. An edited one takes the place of [was], the rule the editor
     * opened at [at], so the person's order stays as it was.
     *
     * Nothing else changes the rules while an editor is open, except a
     * second window. So [was] is looked for again, at [at] first, since two
     * rules can be alike, and then anywhere. Another rule is never
     * overwritten, and an edit whose rule is gone is kept as a new rule.
     * Pure, so the rule is tested without a device.
     */
    fun saved(rules: List<Rule>, rule: Rule, was: Rule?, at: Int?): List<Rule> {
        val place = when {
            was == null -> -1
            at != null && rules.getOrNull(at) == was -> at
            else -> rules.indexOf(was)
        }
        return if (place < 0) rules + rule else rules.toMutableList().apply { set(place, rule) }
    }

    fun save(context: Context, rules: List<Rule>) {
        relayPrefs(context).edit().putString(KEY_RULES, encode(rules)).apply()
    }
}

/**
 * The one preferences file the relay keeps: rules, events, and the last
 * arrival.
 *
 * It keeps the name 1.0.0 gave it, so an update finds the old rules where
 * that version left them.
 */
internal fun relayPrefs(context: Context): SharedPreferences =
    context.getSharedPreferences("relay", Context.MODE_PRIVATE)

/**
 * The objects of a stored JSON array, each turned into a [T] by [read].
 *
 * Text that is not a JSON array reads as empty, and an item that is not an
 * object, or that [read] returns null for, is skipped. The rules and the
 * events are both read this way, so neither can crash the receiver.
 */
internal fun <T : Any> readArray(json: String, read: (JSONObject) -> T?): List<T> {
    val array = try {
        JSONArray(json)
    } catch (e: JSONException) {
        return emptyList()
    }
    return (0 until array.length()).mapNotNull { i -> array.optJSONObject(i)?.let(read) }
}

/**
 * A text field, or null when it is missing or is not text.
 *
 * `getString` is not used. Given a number, Android's org.json returns it as
 * text and the JVM's throws, so the tests would not show what the phone does.
 */
internal fun JSONObject.text(key: String): String? = opt(key) as? String
