package dev.bandbridge.relay

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * Brings the listener back when Android has stopped it, so a press works
 * again without opening anything: after the phone restarts, and after this
 * app is updated, which ends its process and its service.
 *
 * Both broadcasts let an app start a foreground service from the background,
 * which Android 12 and later otherwise refuse. Not every boot broadcast does.
 * Newer Android also sends one when an app leaves the stopped state, such as
 * when a press wakes a relay that was force-stopped, and Android 16 refused
 * the start then. A refusal is caught and logged, never allowed to end the
 * relay: the window starts the service again at its next resume, and a real
 * boot still starts it.
 */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action !in STARTS) {
            return
        }
        try {
            RelayService.start(context)
        } catch (e: IllegalStateException) {
            // Android 12 and later refuse with ForegroundServiceStartNotAllowedException,
            // and Android 8 to 11 with a plain IllegalStateException. The first
            // extends the second, so one catch takes both.
            Log.w(TAG, "Android refused to start the relay's service for ${intent.action}: $e")
        }
    }

    private companion object {
        /** A restart of the phone, and an update of this app, as the manifest lists them. */
        val STARTS = setOf(Intent.ACTION_BOOT_COMPLETED, Intent.ACTION_MY_PACKAGE_REPLACED)

        /** The receiver's tag in the log. */
        const val TAG = "BootReceiver"
    }
}
