package dev.bandbridge.relay

import android.content.Context
import android.content.pm.PackageManager
import android.os.Build

/** An app the relay cannot work without. */
enum class RequiredApp {
    /** Sends the band's events to this app. */
    Gadgetbridge,

    /** Runs the key maps the rules name. */
    KeyMapper,
}

/**
 * Which required apps are missing, so the app can open on Setup until both
 * are installed.
 *
 * The question is asked through a function, so the rule is tested without a
 * device and a debug fixture can pretend.
 */
object Gate {
    /**
     * The Gadgetbridge builds, any one of which opens the gate. The manifest's
     * `<queries>` names each, because Android 11 and later hide an app that is
     * not named there.
     */
    val GADGETBRIDGE: List<String> = listOf(
        "nodomain.freeyourgadget.gadgetbridge",
        "nodomain.freeyourgadget.gadgetbridge.nightly",
        "nodomain.freeyourgadget.gadgetbridge.banglejs",
        "nodomain.freeyourgadget.gadgetbridge.nightly_nopebble",
    )

    const val KEY_MAPPER = BandReceiver.KEY_MAPPER_PACKAGE

    fun missing(installed: (String) -> Boolean): Set<RequiredApp> = buildSet {
        if (GADGETBRIDGE.none(installed)) {
            add(RequiredApp.Gadgetbridge)
        }
        if (!installed(KEY_MAPPER)) {
            add(RequiredApp.KeyMapper)
        }
    }

    /**
     * Which required apps are missing on this phone, a debug fixture's
     * pretend state first.
     *
     * The window asks this to choose its root, and Setup to mark each app.
     * One answer for both means Setup can never be the root with both apps
     * checked, nor Main with one missing.
     */
    fun missing(context: Context): Set<RequiredApp> =
        missing { Fixtures.pretendInstalled(context, it) ?: isInstalled(context, it) }

    private fun isInstalled(context: Context, pkg: String): Boolean = try {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            context.packageManager.getPackageInfo(pkg, PackageManager.PackageInfoFlags.of(0))
        } else {
            @Suppress("DEPRECATION")
            context.packageManager.getPackageInfo(pkg, 0)
        }
        true
    } catch (e: PackageManager.NameNotFoundException) {
        false
    }
}
