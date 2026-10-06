package dev.bandbridge.relay

import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * Gadgetbridge's band events, turned into Key Mapper triggers.
 *
 * Two copies of this receiver hear an event: the one the manifest declares,
 * which Android starts for a broadcast aimed at this app, and the one
 * [RelayService] registers, which also hears a broadcast aimed at nobody. So
 * one event often arrives twice. [Relay.handle] decides what each delivery
 * does, the second one included, and this class only carries that out.
 */
class BandReceiver : BroadcastReceiver() {
    companion object {
        /** What Gadgetbridge broadcasts for every band notification. */
        const val GADGETBRIDGE_NOTIFICATION =
            "nodomain.freeyourgadget.gadgetbridge.ble_api.events.CHARACTERISTIC_CHANGED"

        /** The bytes, hex, upper case. Gadgetbridge names it this on the way out. */
        const val EXTRA_PAYLOAD = "EXTRA_PAYLOAD"

        /**
         * Marks the check's test event, which only this app sends.
         *
         * An event that carries it never runs a key map and never becomes an
         * event row, whether a capture takes it or not (D10).
         */
        const val EXTRA_TEST = "dev.bandbridge.relay.EXTRA_TEST"

        const val KEY_MAPPER_PACKAGE = "io.github.sds100.keymapper"
        private const val KEY_MAPPER_RECEIVER =
            "io.github.sds100.keymapper.api.TriggerKeyMapsBroadcastReceiver"
        private const val KEY_MAPPER_ACTION =
            "io.github.sds100.keymapper.ACTION_TRIGGER_KEYMAP_BY_UID"
        private const val KEY_MAPPER_EXTRA_UID =
            "io.github.sds100.keymapper.EXTRA_KEYMAP_UID"

        /** Sent to this app's own screen, so an open window shows an event live. */
        const val ACTION_SEEN = "dev.bandbridge.relay.SEEN"

        /** The receiver's tag in the log. */
        private const val TAG = "BandReceiver"

        /**
         * Ask Key Mapper to run [rule]'s key map.
         *
         * The component is named rather than left to the action alone. Android
         * will not deliver this to Key Mapper otherwise, and says nothing when
         * it does not.
         *
         * A broadcast that Android refuses to send is logged and dropped, such
         * as one whose id is too large to carry. Thrown on, it would end the
         * relay's process at every press of that button, before the rules
         * after it are sent. The log names the rule's signal and the length of
         * its id, never the id. Android's message names the intent but not its
         * extras, so the id stays out of it too.
         */
        fun trigger(context: Context, rule: Rule) {
            val intent = Intent(KEY_MAPPER_ACTION).apply {
                component = ComponentName(KEY_MAPPER_PACKAGE, KEY_MAPPER_RECEIVER)
                putExtra(KEY_MAPPER_EXTRA_UID, rule.keyMapUid)
            }
            try {
                context.sendBroadcast(intent)
            } catch (e: RuntimeException) {
                Log.w(
                    TAG,
                    "Key Mapper was not sent the key map of a ${rule.signal} rule, whose id is ${rule.keyMapUid.length} characters long",
                    e,
                )
            }
        }

        /**
         * Send this app the broadcast Gadgetbridge sends for a `bb01` press,
         * marked as a test (D10).
         *
         * It skips Gadgetbridge, and is aimed at this package as Gadgetbridge
         * aims a press when its BLE API package field names this app. When it
         * arrives, such a press would arrive too, so a press that does not is
         * lost before it gets here: in Gadgetbridge or its settings.
         */
        fun sendTestEvent(context: Context) {
            val intent = Intent(GADGETBRIDGE_NOTIFICATION)
                .setPackage(context.packageName)
                .putExtra(EXTRA_PAYLOAD, TEST_PAYLOAD)
                .putExtra(EXTRA_TEST, true)
            context.sendBroadcast(intent)
        }
    }

    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != GADGETBRIDGE_NOTIFICATION) {
            return
        }
        val payload = intent.getStringExtra(EXTRA_PAYLOAD) ?: return
        val test = intent.getBooleanExtra(EXTRA_TEST, false)
        val now = System.currentTimeMillis()
        val handling = Relay.handle(payload, test, now, Rules.load(context))
        if (handling == Handling.Duplicate || handling == Handling.BandTraffic || handling == Handling.TestDropped) {
            return
        }

        // Every press counts, a captured one too. The band's own traffic has
        // returned above, so Setup's last press line means a press (C5).
        if (!test) {
            Events.markArrival(context, now)
        }
        if (handling is Handling.Send) {
            for (rule in handling.matched) {
                trigger(context, rule)
            }
            Events.add(context, Event(now, payload, handling.matched.map { it.name }))
        }

        // Tell an open window, which only redraws. Firing Key Mapper stays
        // here, so a window that happens to be open cannot double a press.
        context.sendBroadcast(Intent(ACTION_SEEN).setPackage(context.packageName))
    }
}
