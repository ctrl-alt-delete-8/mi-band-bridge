package dev.bandbridge.relay

/**
 * How long Learn and the check listen for a press.
 *
 * A design choice, not a platform limit. Both screens read this one value, so
 * the two countdowns cannot drift apart. The countdown has one segment for
 * each second.
 */
const val LISTEN_SECONDS = 10

/**
 * How long the check waits for its own test event.
 *
 * The event never leaves the phone, so it arrives in far less time. When it
 * has not arrived by then, the fault is in this app.
 */
const val TEST_EVENT_WAIT_MS = 3000L

/**
 * The payload of the check's test event: a `bb01` press, as Gadgetbridge
 * sends it.
 *
 * It is a real press byte for byte, so it goes through the same parse and
 * the same receivers as one. Only [BandReceiver.EXTRA_TEST] tells it apart.
 */
const val TEST_PAYLOAD = "0303004F0002000000A000BB01"
