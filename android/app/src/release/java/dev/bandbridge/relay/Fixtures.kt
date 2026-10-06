package dev.bandbridge.relay

import android.content.Context
import android.content.Intent
import dev.bandbridge.relay.ui.Navigator
import dev.bandbridge.relay.ui.Page
import dev.bandbridge.relay.ui.Screen

/**
 * The debug build's fixtures, doing nothing (D17, D18).
 *
 * The APK that goes to the reader carries no example data and nothing that
 * pretends. It only needs the same functions to exist, so MainActivity is
 * the same in both builds.
 */
object Fixtures {
    fun apply(context: Context, intent: Intent) = Unit

    /** Always the real install state. */
    fun pretendInstalled(context: Context, pkg: String): Boolean? = null

    /** The check's test event is always sent. */
    fun dropTestEvent(context: Context): Boolean = false

    /** Every page is the real one. */
    fun page(context: Context, screen: Screen, navigator: Navigator): Page? = null
}
