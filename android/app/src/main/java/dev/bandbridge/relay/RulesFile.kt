package dev.bandbridge.relay

import java.io.InputStream
import java.time.Instant
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.temporal.ChronoUnit
import org.json.JSONArray
import org.json.JSONException
import org.json.JSONObject
import org.json.JSONTokener

/**
 * The rules file that the ⋮ menu exports and imports (the note, section 8):
 * one JSON object with the format's name, its version, the moment it was
 * written, and the rules in list order.
 *
 * ```
 * { "format": "bandbridge-relay-rules", "version": 1,
 *   "exportedAt": "2026-09-27T09:42:10Z",
 *   "rules": [ { "signal": "bb01", "name": "Page down", "keyMapId": "3f2a…" } ] }
 * ```
 *
 * The file calls a rule's key map `keyMapId`, as the note does, while
 * [Rule] keeps the name `keyMapUid`. A file comes from outside the app and
 * may be anything, so reading it never throws: what is not the format, in
 * any way, is [Read.NotARulesFile]. Pure, so it is tested without a device.
 */
object RulesFile {
    const val FORMAT = "bandbridge-relay-rules"
    const val VERSION = 1

    /** A rules file is small. One of more bytes is something else, and is not read to its end. */
    const val MAX_BYTES = 1_048_576

    /**
     * The most rules a file may hold, and the most an import fills the list
     * to. No band needs near this many.
     *
     * The byte limit alone is not enough: 1 MB holds some 20,000 short rules,
     * and the main page draws a row for each. On the emulator a list of
     * 20,000 ran the window out of memory as it drew it, at every start
     * after the import, while 1,000 rows took about 30 MB and 2 s.
     */
    const val MAX_RULES = 1_000

    /**
     * The most characters a rule's signal, name or key map id may hold, as
     * the list keeps it, trimmed. A Key Mapper id is 36, and no band needs
     * a longer name.
     *
     * An entry with a longer field is skipped, as a broken one is, since one
     * long field is enough to break the app for good. On the emulator a name
     * of 1,000,000 characters took the window about 2 minutes to lay out, at
     * every start, until Android called it not responding. A key map id of
     * 600,000 was too large for the broadcast that sends it to Key Mapper,
     * which ended the relay at every press. A name of 100,000 alone added
     * 1.8 s to every start. The rule editor's fields take no more than this,
     * so a paste cannot store what an import refuses.
     */
    const val MAX_FIELD_LENGTH = 100

    /** What some editors put before UTF-8 text. */
    private const val BYTE_ORDER_MARK = "\uFEFF"

    /** What org.json's nextClean answers when only blank space is left. */
    private const val END_OF_TEXT = '\u0000'

    /** What a file turned out to be. */
    sealed interface Read {
        /** A rules file, and the rules in it that can be used, in its order. */
        data class Rules(val rules: List<Rule>) : Read

        /** Anything else: another format, a newer version, broken JSON, or too long, in bytes or in rules. */
        data object NotARulesFile : Read
    }

    /**
     * The file for [rules], in their order, written at [exportedAt].
     *
     * The time is UTC to the second, as the note writes it, and the text is
     * indented, since a person may open the file to see what it holds.
     */
    fun write(rules: List<Rule>, exportedAt: Instant): String {
        val list = JSONArray()
        for (rule in rules) {
            list.put(
                JSONObject()
                    .put("signal", rule.signal)
                    .put("name", rule.name)
                    .put("keyMapId", rule.keyMapUid),
            )
        }
        val file = JSONObject()
            .put("format", FORMAT)
            .put("version", VERSION)
            .put("exportedAt", DateTimeFormatter.ISO_INSTANT.format(exportedAt.truncatedTo(ChronoUnit.SECONDS)))
            .put("rules", list)
        return file.toString(2) + "\n"
    }

    /**
     * What [text] is (B4).
     *
     * A rules file is one JSON object with this [FORMAT], [VERSION] 1, and a
     * `rules` list of at most [MAX_RULES] entries, and nothing after it but
     * blank space. A byte order mark before it is ignored: some editors write
     * one when they save UTF-8. In a rules file, an entry that is not an
     * object, or that lacks a signal, a name or a key map id as text that is
     * not blank, or holds one longer than [MAX_FIELD_LENGTH], is skipped, and
     * the others are kept. Each is trimmed, and its signal normalized as the
     * list keeps it.
     *
     * org.json reads a nested list or object by calling itself, so a small
     * hostile file of brackets would overflow the stack. That file is not a
     * rules file either, and the window that reads it goes on.
     */
    fun read(text: String): Read {
        val root = try {
            val tokener = JSONTokener(text.removePrefix(BYTE_ORDER_MARK))
            val value = tokener.nextValue()
            if (tokener.nextClean() != END_OF_TEXT) {
                return Read.NotARulesFile
            }
            value
        } catch (e: JSONException) {
            return Read.NotARulesFile
        } catch (e: StackOverflowError) {
            return Read.NotARulesFile
        }
        if (root !is JSONObject || root.opt("format") != FORMAT || root.opt("version") != VERSION) {
            return Read.NotARulesFile
        }
        val list = root.opt("rules") as? JSONArray ?: return Read.NotARulesFile
        if (list.length() > MAX_RULES) {
            return Read.NotARulesFile
        }
        return Read.Rules((0 until list.length()).mapNotNull { i -> (list.opt(i) as? JSONObject)?.let(::ruleOf) })
    }

    /**
     * Reads at most MAX_BYTES; a longer stream is NotARulesFile.
     *
     * The bytes are UTF-8. To tell a file of exactly the limit from a
     * longer one, one byte more is asked for, and never any after it. A
     * failure to read is the caller's to report, so its IOException is not
     * caught here. The caller closes the stream.
     */
    fun read(stream: InputStream): Read {
        val bytes = ByteArray(MAX_BYTES + 1)
        var length = 0
        while (length < bytes.size) {
            val count = stream.read(bytes, length, bytes.size - length)
            if (count < 0) {
                break
            }
            length += count
        }
        if (length > MAX_BYTES) {
            return Read.NotARulesFile
        }
        return read(String(bytes, 0, length, Charsets.UTF_8))
    }

    /**
     * The existing rules, then each incoming one that is not an exact copy, in order.
     *
     * Import only adds, by the person's decision: a rule already in the list
     * is never changed or removed. A copy is judged after the trimming and
     * normalizing that [read] does, and one that repeats an earlier rule of
     * the same file counts once. New rules go at the end, as they come,
     * until the list holds [MAX_RULES], so that no number of imports can
     * make it too long to draw.
     */
    fun merged(existing: List<Rule>, incoming: List<Rule>): List<Rule> {
        val seen = existing.mapTo(HashSet(), ::tidied)
        val room = (MAX_RULES - existing.size).coerceAtLeast(0)
        return existing + incoming.map(::tidied).filter { seen.add(it) }.take(room)
    }

    /**
     * The name Android's Save dialog suggests: mi-band-bridge-rules-2026-09-27.json.
     * Before 2.4.0 it was bandbridge-rules-2026-09-27.json. An import reads
     * either, since it reads what is in a file, whatever its name.
     */
    fun fileName(date: LocalDate): String = "mi-band-bridge-rules-${DateTimeFormatter.ISO_LOCAL_DATE.format(date)}.json"

    /**
     * The files Android's Open dialog offers for an import: all of them. A
     * browser or a messenger may have saved a rules file under another type,
     * or another name, so what is in the file decides.
     */
    const val OPEN_TYPE = "*/*"

    /**
     * One entry of the file as a rule, or null when it lacks a field (B4),
     * or holds one longer than [MAX_FIELD_LENGTH]. The length is judged on
     * the rule as the list would keep it.
     */
    private fun ruleOf(entry: JSONObject): Rule? {
        val signal = entry.text("signal")?.trim()
        val name = entry.text("name")?.trim()
        val keyMapUid = entry.text("keyMapId")?.trim()
        if (signal.isNullOrEmpty() || name.isNullOrEmpty() || keyMapUid.isNullOrEmpty()) {
            return null
        }
        val rule = tidied(Rule(signal, name, keyMapUid))
        return rule.takeIf { maxOf(it.signal.length, it.name.length, it.keyMapUid.length) <= MAX_FIELD_LENGTH }
    }

    /**
     * [rule] as the list keeps it: its name and id trimmed, and its signal
     * normalized. A signal that does not follow the signal rule is kept as
     * typed, trimmed and in lower case, as the move from 1.0.0 keeps one
     * (D5), so the person can see it and fix it.
     */
    private fun tidied(rule: Rule): Rule = Rule(
        Signals.normalize(rule.signal) ?: rule.signal.trim().lowercase(),
        rule.name.trim(),
        rule.keyMapUid.trim(),
    )
}
