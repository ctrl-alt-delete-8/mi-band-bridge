package dev.bandbridge.relay

import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

/** A press today, to the second: 09:41:08. */
private val TIME: DateTimeFormatter = DateTimeFormatter.ofPattern("HH:mm:ss", Locale.US)

/** A press on an earlier day, to the minute: Sep 25 at 09:41. */
private val DAY_AND_TIME: DateTimeFormatter = DateTimeFormatter.ofPattern("MMM d 'at' HH:mm", Locale.US)

/**
 * Lines the pages say, worked out from what the person typed or what the
 * relay stored, apart from any view so they are tested without a device.
 */
object Words {
    /**
     * The line above the rule editor's Save, or null when the rule can be
     * saved (the note, section 3, and D11).
     *
     * An empty field is named first, all of them in one line, in the order
     * the form lists them: "Add a signal and a key map id to save." Only
     * when all three are there does the signal rule come up. It is the rule
     * `src/signal.rs` applies on the Mac, so a signal saved here works on
     * both. Save stays disabled while there is a line.
     */
    fun missing(signal: String, name: String, keyMapUid: String): String? {
        val empty = listOfNotNull(
            "a signal".takeIf { signal.isBlank() },
            "a name".takeIf { name.isBlank() },
            "a key map id".takeIf { keyMapUid.isBlank() },
        )
        return when {
            empty.isNotEmpty() -> "Add ${series(empty)} to save."
            Signals.normalize(signal) == null -> "A signal is hex that starts with bb, such as bb01."
            else -> null
        }
    }

    /**
     * Setup's line under "Gadgetbridge is installed" (D12): when the last
     * real band event arrived, [at], or null before the first.
     *
     * It says whether Gadgetbridge sends anything at all, so it reads the
     * time the receiver stores for every real event, which Clear does not
     * wipe. A press today reads as its time, to the second. One from an
     * earlier day reads as its date and minute, since a time alone would
     * pass for today's. Today is the date in [zone] at [now], not the last
     * 24 hours: a press just before midnight arrived yesterday.
     */
    fun lastPress(at: Long?, now: Long, zone: ZoneId = ZoneId.systemDefault()): String {
        if (at == null) {
            return "No band press has arrived yet."
        }
        val then = Instant.ofEpochMilli(at).atZone(zone)
        return if (then.toLocalDate() == Instant.ofEpochMilli(now).atZone(zone).toLocalDate()) {
            "Last band press arrived at ${TIME.format(then)}."
        } else {
            "Last band press arrived on ${DAY_AND_TIME.format(then)}."
        }
    }

    /**
     * The check's line when a band app press arrived (frame 12): its [label]
     * and the time it arrived, [at], to the second, in [zone].
     *
     * A press proves that Gadgetbridge sends to this app, so the line names
     * the press as its event row would, by its signal, such as bb01. The
     * relay hands the check no other event but its own test (A1).
     */
    fun arrived(label: String, at: Long, zone: ZoneId = ZoneId.systemDefault()): String =
        "$label arrived at ${TIME.format(Instant.ofEpochMilli(at).atZone(zone))}."

    /**
     * The line under the Rules header after an export (B5): how many rules
     * went into the [file], by the name Android's Save dialog gave it.
     */
    fun exported(count: Int, file: String): String = "Exported ${rules(count)} to $file."

    /**
     * The line under the Rules header after an import (B5): how many rules
     * the [file] added. Copies of rules already there are not counted, so it
     * can read "0 rules".
     */
    fun imported(count: Int, file: String): String = "Imported ${rules(count)} from $file."

    /**
     * Setup's notice under the required apps (A3): the relay hears only a
     * custom band app, which the desktop app installs. The user's words of
     * 2026-09-28, with the release page's address on its own line after
     * them in place of "<download link>". The notice opens it.
     */
    const val DESKTOP_APP_NOTICE = "The band also needs a custom band app. Install one with the desktop app from:"

    /**
     * The notice under it (to-do 17): without the desktop app, the sample
     * band app goes on the band with Gadgetbridge, from the same page. One
     * plain sentence, not "No desktop app yet?", by the user's rule for UI
     * words (2026-09-28).
     */
    const val SAMPLE_APP_NOTICE = "Without the desktop app, download the sample band app, BandArrows, from the same page, and install it with Gadgetbridge."

    /** The note's line for a file that is not the format, a newer version, broken, or too long. */
    const val NOT_A_RULES_FILE = "This file is not a Mi Band Bridge rules file. Nothing was imported."

    /** B5: the export's file could not be written, such as when the disk is full or the grant is gone. */
    const val NOT_WRITTEN = "Could not write the file. Nothing was exported."

    /** B5: the import's file could not be read, such as when it went away after it was picked. */
    const val NOT_READ = "Could not read the file. Nothing was imported."

    /** B5: an e-ink reader may have no app that answers Android's Save dialog. */
    const val NO_APP_TO_SAVE = "This device has no app to save files."

    /** B5: nor one that answers its Open dialog. */
    const val NO_APP_TO_OPEN = "This device has no app to open files."

    /**
     * [line] as the notice under the Rules header shows it: the name of the
     * [file] it ends with starts a line of its own, as frame 17 breaks it.
     * Left to wrap, the line would break after a hyphen inside the name,
     * such as "bandbridge-", and split it. A name too long for one line
     * still wraps, on lines of its own. A line without the name is returned
     * as it is.
     */
    fun fileOnItsOwnLine(line: String, file: String): String {
        val at = line.lastIndexOf(" $file")
        return if (at < 0) line else line.replaceRange(at, at + 1, "\n")
    }

    /** "1 rule", or "3 rules", and "0 rules". */
    private fun rules(count: Int): String = if (count == 1) "1 rule" else "$count rules"

    /** "a", "a and b", or "a, b, and c", with the serial comma. */
    private fun series(items: List<String>): String = when (items.size) {
        1 -> items[0]
        2 -> "${items[0]} and ${items[1]}"
        else -> items.dropLast(1).joinToString(", ") + ", and " + items.last()
    }
}
