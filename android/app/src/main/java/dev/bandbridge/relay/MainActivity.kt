package dev.bandbridge.relay

import android.Manifest
import android.app.Activity
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.os.Build
import android.os.Bundle
import android.util.Log
import android.widget.FrameLayout
import dev.bandbridge.relay.ui.CheckScreen
import dev.bandbridge.relay.ui.MainScreen
import dev.bandbridge.relay.ui.Navigator
import dev.bandbridge.relay.ui.Page
import dev.bandbridge.relay.ui.RuleEditorScreen
import dev.bandbridge.relay.ui.Screen
import dev.bandbridge.relay.ui.SetupScreen

/**
 * The app's one window. It hosts the navigator and keeps it in step with the
 * gate, the stored data, System Back, and its own lifecycle.
 *
 * A rotation or a resize does not recreate it (D2): the manifest takes those
 * changes, and the navigator lays the same pages out again, so an armed
 * capture survives a turn of the screen.
 */
class MainActivity : Activity() {
    private lateinit var navigator: Navigator

    /** Redraws when the receiver has handled an event. */
    private val onSeen = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) = navigator.refresh()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // A window restored after the process died carries its first launch
        // intent again. Applying its fixture twice would wipe what was done
        // since.
        if (savedInstanceState == null) {
            Fixtures.apply(this, intent)
        }
        RelayService.start(this)
        val host = FrameLayout(this)
        setContentView(host)
        val first = if (Gate.missing(this).isEmpty()) Screen.Main else Screen.Setup(asRoot = true)
        navigator = Navigator(this, host, first, ::page)
    }

    override fun onStart() {
        super.onStart()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            registerReceiver(onSeen, IntentFilter(BandReceiver.ACTION_SEEN), Context.RECEIVER_NOT_EXPORTED)
        } else {
            @Suppress("UnspecifiedRegisterReceiverFlag")
            registerReceiver(onSeen, IntentFilter(BandReceiver.ACTION_SEEN))
        }
    }

    override fun onStop() {
        super.onStop()
        unregisterReceiver(onSeen)
    }

    /**
     * The gate, asked again each time the window comes back, since the
     * person may have installed or removed an app meanwhile. Setup is the
     * root while an app is missing, and Main replaces it, never stacked on
     * it, once both are there.
     *
     * The service is started again too, whenever notifications are allowed.
     * Android shows no notification for a foreground service that started
     * before the permission was granted, and does not post it when the
     * permission arrives, whether through the ask of D15 or later in
     * Settings. A start posts "Band relay", which Setup's step 6 asks the
     * person to keep, and posting it again changes nothing.
     */
    override fun onResume() {
        super.onResume()
        val gate = Screen.Setup(asRoot = true)
        val missing = Gate.missing(this)
        when {
            missing.isNotEmpty() && navigator.root != gate -> navigator.replaceRoot(gate)
            missing.isEmpty() && navigator.root == gate -> navigator.replaceRoot(Screen.Main)
            else -> navigator.refresh()
        }
        navigator.resume()
        if (notificationsAllowed()) {
            startServiceAgain()
        }
        askForNotificationsOnce()
    }

    /**
     * The start in [onResume]. Android can refuse it right after the screen
     * unlocks, before it counts the app as in front again, and throws: Android
     * 9 did so for a plain start. The next resume starts the service, so the
     * refusal is logged rather than allowed to stop the app.
     */
    private fun startServiceAgain() {
        try {
            RelayService.start(this)
        } catch (e: IllegalStateException) {
            Log.w(TAG, "Android refused to start the relay's service on resume", e)
        }
    }

    /** Below Android 13 an app needs no permission to post a notification. */
    private fun notificationsAllowed(): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

    /**
     * D15: on Android 13 and later, the first time Main is shown, ask for
     * the notification permission, unless it is already granted.
     *
     * Without it there is no "Band relay" notification, which Setup's step 6
     * and the check's last result speak of. The ask is remembered before it
     * is made, so neither the resume after the system's dialog nor a later
     * one asks again, whatever the person answered: from then on it is
     * theirs to change in Settings.
     */
    private fun askForNotificationsOnce() {
        if (notificationsAllowed() || navigator.root != Screen.Main) {
            return
        }
        val store = getPreferences(Context.MODE_PRIVATE)
        if (store.getBoolean(KEY_ASKED_NOTIFICATIONS, false)) {
            return
        }
        store.edit().putBoolean(KEY_ASKED_NOTIFICATIONS, true).apply()
        requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), ASK_NOTIFICATIONS)
    }

    override fun onPause() {
        navigator.pause()
        super.onPause()
    }

    /**
     * Android's Save or Open dialog, started by the main page's ⋮ menu,
     * answered. Main writes or reads the file the person chose, and says how
     * it went. A dialog left without a choice changes nothing.
     *
     * This runs before [onResume], so the page has its line before the
     * window is back in front. If Android ended the process meanwhile, the
     * window was built again, and its new Main takes the answer.
     */
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        val file = data?.data
        if (resultCode != RESULT_OK || file == null) {
            return
        }
        (navigator.pageOf(Screen.Main) as? MainScreen)?.fileChosen(requestCode, file)
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        if (!navigator.back()) {
            @Suppress("DEPRECATION")
            super.onBackPressed()
        }
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        navigator.relayout()
    }

    /** Builds the page for [screen]. A debug fixture may put its own first, such as the widget sheet. */
    private fun page(screen: Screen, navigator: Navigator): Page =
        Fixtures.page(this, screen, navigator) ?: when (screen) {
            Screen.Main -> MainScreen(this, navigator)
            is Screen.RuleEditor -> RuleEditorScreen(this, navigator, screen.at, screen.prefill)
            is Screen.Setup -> SetupScreen(this, navigator, screen.asRoot)
            Screen.Check -> CheckScreen(this, navigator)
        }
}

/** The window's tag in the log. */
private const val TAG = "MainActivity"

/** In MainActivity's own preferences: whether the notification permission was asked for (D15). */
private const val KEY_ASKED_NOTIFICATIONS = "asked_notifications"

/** The request code of that one ask. */
private const val ASK_NOTIFICATIONS = 1
