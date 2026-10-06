package dev.bandbridge.relay

/**
 * Something a person did in a custom band app (A1). Anything else the band
 * sends is its own traffic with Gadgetbridge, which the relay drops (C2).
 *
 * A band app's signal is the one kind of press, because the user chose to
 * take presses from a band app only. The band's own buttons, the keys of
 * its music screen among them, were a second kind in 2.2.0 and are traffic
 * now.
 */
sealed interface Press {
    /** A band app sent [signal], such as bb01. A rule can name it and run a key map. */
    data class Signal(val signal: String) : Press
}

/**
 * What a Gadgetbridge payload says, and which rules it fires.
 *
 * A payload is one chunk of the band's protocol, in hex, as Gadgetbridge
 * broadcasts it. Most chunks are the band's own traffic with Gadgetbridge:
 * acknowledgements, encrypted requests, and replies. One kind is a press
 * (A1): a first chunk on the app endpoint, in clear, whose body follows the
 * signal rule of `src/signal.rs`. The band's own buttons are traffic, the
 * keys of its music screen included, though they come in clear. The chunk
 * also carries a handle that changes between messages, so a press is read
 * field by field, never matched as a whole string.
 *
 * See "Chunk framing" in `docs/PROTOCOL.md`, and "Music endpoint" for the
 * music keys.
 */
object Signals {
    /** Byte 0 of every chunk the band sends. */
    private const val CHUNK_MARKER = 0x03

    /** Flag bits of byte 1. */
    private const val FLAG_FIRST = 0x01
    private const val FLAG_ENCRYPTED = 0x08

    /** A first chunk's header: marker, flags, extended header, handle, counter, length, endpoint. */
    private const val HEADER_SIZE = 11

    /** The endpoint a band app sends on. */
    private const val APP_ENDPOINT = 0x00a0

    /** The first byte of every signal, as `src/signal.rs` defines it. */
    private const val PREFIX = 0xbb

    /** The prefix, then one to seven bytes. */
    private val SIGNAL_SIZE = 2..8

    /** A signal is written in lower case hex, as the Mac writes it. */
    private const val HEX_DIGITS = "0123456789abcdef"

    /** The chip of an event whose payload is empty. */
    private const val NO_BYTES = "—"

    /** Two deliveries of one payload inside this window are one press. */
    private const val SAME_PRESS_MS = 800L

    /** Each payload seen in the last [SAME_PRESS_MS], and when it was first seen. */
    private val sightings = HashMap<String, Long>()

    /**
     * Lower case hex when the text follows the signal rule of src/signal.rs,
     * else null.
     *
     * Surrounding spaces and upper case are accepted, as the Mac accepts them,
     * so `BB04` and `bb04` are one signal. A signal typed here therefore works
     * on the Mac too, and the other way around.
     */
    fun normalize(text: String): String? {
        val bytes = bytesOf(text.trim()) ?: return null
        return if (followsTheRule(bytes)) hexOf(bytes) else null
    }

    /**
     * What a person did in a band app, or null for the band's own traffic
     * (A1).
     *
     * A press is a first chunk in clear on the app endpoint, whose body
     * follows the signal rule. Its body is as many bytes as the length field
     * says, and bytes past that length are ignored, as the Mac's decoder
     * ignores them. Everything else is traffic: encrypted chunks,
     * continuation chunks, acknowledgements, other endpoints, the music
     * screen's keys and its opening or closing, and the app endpoint's
     * replies to Gadgetbridge. A payload that is not hex, or is cut short, is
     * no press and throws nothing: whatever Gadgetbridge sends must not crash
     * the receiver.
     */
    fun pressOf(payload: String): Press? {
        val chunk = firstChunkInClear(payload) ?: return null
        if (chunk.endpoint != APP_ENDPOINT || !followsTheRule(chunk.body)) {
            return null
        }
        return Press.Signal(hexOf(chunk.body))
    }

    /**
     * The signal of a band app press, such as `bb01`, or null for anything
     * else (D7). Only a signal can match a rule.
     */
    fun signalOf(payload: String): String? = (pressOf(payload) as? Press.Signal)?.signal

    /**
     * The short name an event row shows in its chip.
     *
     * A band app press shows its signal. Any other payload shows its first
     * two bytes as it writes them, such as `0303`, because those say what
     * kind of chunk it is (D8). The relay records no such payload now (A1),
     * but a row that an earlier version stored still draws: a row of the
     * band's own traffic from 2.1.0, or of a music key from 2.2.0, whose chip
     * reads `0303` again where 2.2.0 showed `vol+`. An empty payload shows a
     * dash, so the chip is never blank.
     */
    fun labelOf(payload: String): String =
        signalOf(payload) ?: payload.trim().let { if (it.isEmpty()) NO_BYTES else it.take(4) }

    /**
     * Every rule on this payload's signal, in list order, once per key map id,
     * skipping blank ids.
     *
     * One signal may drive several key maps. Give each one an "app in
     * foreground" constraint in Key Mapper, and the same band button does a
     * different thing in each app. Key Mapper checks a key map's constraints
     * when another app triggers it, and skips it silently when they fail, so
     * the relay sends them all and lets Key Mapper choose.
     *
     * A rule with no key map id is skipped rather than matched, so a half
     * made rule does not look like a broken band. A key map listed twice for
     * one signal is sent once, because it would otherwise run twice. Each rule
     * comes back with its signal normalized and its key map id trimmed, which
     * is the id Key Mapper is sent.
     */
    fun matchAll(payload: String, rules: List<Rule>): List<Rule> {
        val signal = signalOf(payload) ?: return emptyList()
        return rules
            .filter { it.keyMapUid.isNotBlank() && normalize(it.signal) == signal }
            .map { it.copy(signal = signal, keyMapUid = it.keyMapUid.trim()) }
            .distinctBy { it.keyMapUid }
    }

    /**
     * True the first time a payload is seen, false for a repeat inside the
     * window.
     *
     * The manifest receiver and the service both hear a broadcast that names
     * this app, and firing Key Mapper twice for one press is worse than not
     * firing at all. The payload carries a handle that changes every message,
     * so the same bytes twice inside a moment is one press delivered twice.
     *
     * Every payload of the window is remembered, not only the last one. On
     * Android 13 and older the service's receiver gets a broadcast at once and
     * the manifest receiver gets it later, so another event can land between
     * the two deliveries of a press. The window runs from the first sighting,
     * and each call forgets what has left it. That includes an entry stamped
     * after [now], left by a clock that was set back, which would otherwise
     * hold its payload until the clock caught up. The time is a parameter so
     * a test can set it.
     */
    @Synchronized
    fun isFirstSighting(payload: String, now: Long = System.currentTimeMillis()): Boolean {
        sightings.values.removeAll { seen -> now - seen !in 0 until SAME_PRESS_MS }
        if (payload in sightings) {
            return false
        }
        sightings[payload] = now
        return true
    }

    /** A first chunk's endpoint, and its body as long as its length field says. */
    private class FirstChunk(val endpoint: Int, val body: ByteArray)

    /**
     * The endpoint and body of a first chunk in clear, or null for any other
     * chunk: an encrypted one, a continuation, an acknowledgement, or one
     * shorter than its length field says.
     */
    private fun firstChunkInClear(payload: String): FirstChunk? {
        val bytes = bytesOf(payload.trim()) ?: return null
        if (bytes.size < HEADER_SIZE || bytes.unsigned(0) != CHUNK_MARKER) {
            return null
        }
        val flags = bytes.unsigned(1)
        if (flags and FLAG_FIRST == 0 || flags and FLAG_ENCRYPTED != 0) {
            return null
        }
        // An unsigned 32-bit length. Read into an Int, a large one would turn
        // negative and pass the check below.
        val length = bytes.unsigned(5).toLong() or
            (bytes.unsigned(6).toLong() shl 8) or
            (bytes.unsigned(7).toLong() shl 16) or
            (bytes.unsigned(8).toLong() shl 24)
        if (length > bytes.size - HEADER_SIZE) {
            return null
        }
        val endpoint = bytes.unsigned(9) or (bytes.unsigned(10) shl 8)
        return FirstChunk(endpoint, bytes.copyOfRange(HEADER_SIZE, HEADER_SIZE + length.toInt()))
    }

    private fun followsTheRule(bytes: ByteArray): Boolean =
        bytes.size in SIGNAL_SIZE && bytes.unsigned(0) == PREFIX

    private fun ByteArray.unsigned(at: Int): Int = this[at].toInt() and 0xff

    /** Hex text as bytes, or null when it is not whole bytes of hex. */
    private fun bytesOf(hex: String): ByteArray? {
        if (hex.length % 2 != 0) {
            return null
        }
        val bytes = ByteArray(hex.length / 2)
        for (i in bytes.indices) {
            val high = digitOf(hex[2 * i])
            val low = digitOf(hex[2 * i + 1])
            if (high < 0 || low < 0) {
                return null
            }
            bytes[i] = ((high shl 4) or low).toByte()
        }
        return bytes
    }

    /**
     * One hex digit's value, or -1.
     *
     * `Character.digit` alone also reads the digits of other scripts, such as
     * Arabic-Indic ones, which the Mac refuses. Only ASCII counts, so a signal
     * this app accepts is one the Mac accepts.
     */
    private fun digitOf(char: Char): Int =
        if (char.code < 0x80) Character.digit(char, 16) else -1

    private fun hexOf(bytes: ByteArray): String = buildString(bytes.size * 2) {
        for (byte in bytes) {
            val value = byte.toInt() and 0xff
            append(HEX_DIGITS[value shr 4])
            append(HEX_DIGITS[value and 0x0f])
        }
    }
}
