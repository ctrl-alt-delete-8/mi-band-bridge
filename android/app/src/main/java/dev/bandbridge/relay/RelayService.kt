package dev.bandbridge.relay

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Build
import android.os.IBinder

/**
 * Holds a receiver that is registered in code, for as long as the phone is on.
 *
 * A manifest receiver only ever sees a broadcast aimed at this app by name.
 * Gadgetbridge aims one only when its BLE API package field is filled in, and
 * when that field is empty it broadcasts to everyone instead. Android 8 and
 * later hand such a broadcast to receivers registered in code and to nobody
 * else, so without this service an empty field means silence.
 *
 * The service polls nothing and holds no wake lock. It exists to keep one
 * receiver registered, which costs a notification and no measurable battery.
 */
class RelayService : Service() {
    private val receiver = BandReceiver()
    private var registered = false

    companion object {
        private const val CHANNEL = "relay"

        /**
         * Starts the service, or reaches it again while it runs.
         *
         * It is a foreground start. From Android 8, the minimum here, a plain
         * start from the background can be refused, and after a boot or an
         * update no window is open. A foreground start is allowed then on
         * every version: always up to Android 11, and from Android 12 while
         * the app handles one of the two broadcasts [BootReceiver] takes. It
         * must reach startForeground within seconds, and [onStartCommand]
         * calls it every time.
         *
         * Android can still refuse, with an IllegalStateException, as Android
         * 16 did for the boot broadcast that it sends an app as the app
         * leaves the stopped state. So [BootReceiver] and the window's resume
         * catch it.
         */
        fun start(context: Context) {
            context.startForegroundService(Intent(context, RelayService::class.java))
        }
    }

    override fun onCreate() {
        super.onCreate()
        val filter = IntentFilter(BandReceiver.GADGETBRIDGE_NOTIFICATION)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            registerReceiver(receiver, filter, Context.RECEIVER_EXPORTED)
        } else {
            @Suppress("UnspecifiedRegisterReceiverFlag")
            registerReceiver(receiver, filter)
        }
        registered = true
    }

    /**
     * Posts the notification on every start, not only the first. Android
     * shows none for a service that went to the foreground before the
     * notification permission was granted, and does not post it when the
     * permission arrives. So MainActivity starts the service each time it
     * comes back while notifications are allowed, which covers the ask of
     * D15 and a grant in Settings alike. Posting the same notification
     * again changes nothing.
     */
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        startInForeground()
        return START_STICKY
    }

    override fun onDestroy() {
        super.onDestroy()
        if (registered) {
            unregisterReceiver(receiver)
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun startInForeground() {
        val manager = getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            manager.createNotificationChannel(
                NotificationChannel(CHANNEL, "Band relay", NotificationManager.IMPORTANCE_MIN)
            )
        }
        val notification: Notification = Notification.Builder(this, CHANNEL)
            .setContentTitle("Listening for band presses")
            .setSmallIcon(android.R.drawable.ic_menu_compass)
            .build()
        startForeground(1, notification)
    }
}
