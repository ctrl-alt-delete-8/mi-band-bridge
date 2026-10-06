package dev.bandbridge.relay.ui

import android.content.Context
import android.graphics.Typeface
import dev.bandbridge.relay.R
import kotlin.math.max
import kotlin.math.roundToInt

/**
 * The design's four colors, its unit, and its two typefaces.
 *
 * Black, white and two grays, and nothing else. An e-ink panel shows only a
 * few shades, so meaning is carried by words, weight, borders and filled
 * black marks, even for an error. The design measures in CSS pixels, and one
 * of them is one dp here. One of its font pixels is one sp.
 */
object Ink {
    val BLACK: Int = 0xFF000000.toInt()
    val WHITE: Int = 0xFFFFFFFF.toInt()

    /** Secondary text, such as a label, a hint or a time. */
    val DIM: Int = 0xFF4D4D4D.toInt()

    /** Hairlines between rows, and anything disabled. */
    val HAIRLINE: Int = 0xFF999999.toInt()

    /** The dialog's scrim, black at 30%: the one color with any transparency (frame 03). */
    val SCRIM: Int = 0x4D000000

    private val typefaces = HashMap<Int, Typeface>()

    /**
     * Whole pixels for [value] dp.
     *
     * A value above zero is never less than one pixel, so a hairline cannot
     * vanish on a low density screen.
     */
    fun dp(context: Context, value: Float): Int {
        val px = (value * context.resources.displayMetrics.density).roundToInt()
        return if (value > 0f) max(1, px) else px
    }

    /** Inter, for every word of the interface. The design uses weights 400, 500 and 600 only. */
    fun inter(context: Context, weight: Int): Typeface = font(
        context,
        when (weight) {
            400 -> R.font.inter_regular
            500 -> R.font.inter_medium
            600 -> R.font.inter_semibold
            else -> throw IllegalArgumentException("The design has no Inter $weight")
        },
    )

    /** JetBrains Mono, for signals, ids, times and raw bytes, where each character counts. */
    fun mono(context: Context, weight: Int): Typeface = font(
        context,
        when (weight) {
            400 -> R.font.jetbrains_mono_regular
            500 -> R.font.jetbrains_mono_medium
            600 -> R.font.jetbrains_mono_semibold
            else -> throw IllegalArgumentException("The design has no JetBrains Mono $weight")
        },
    )

    /** Loaded once each: every row of every page asks for the same few fonts. */
    @Synchronized
    private fun font(context: Context, id: Int): Typeface =
        typefaces.getOrPut(id) { context.resources.getFont(id) }
}
