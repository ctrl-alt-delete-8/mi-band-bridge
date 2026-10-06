package dev.bandbridge.relay

import org.junit.Assert.assertEquals
import org.junit.Test

class AboutTest {
    /** The About section names the app and its version, as the desktop apps' About does. */
    @Test
    fun aboutNamesTheAppAndItsVersion() {
        assertEquals("Mi Band Bridge 2.4.0", About.name("2.4.0"))
        assertEquals("Mi Band Bridge", About.name(null))
        assertEquals("Unofficial. Not made by or connected with Xiaomi.", About.UNOFFICIAL)
        assertEquals(
            "The app is free to use, at work too. Its source code is under PolyForm Noncommercial 1.0.0.",
            About.LICENSES,
        )
    }

    /** The desktop app, and the sample band app, are on the release page: this address, exactly. */
    @Test
    fun theDownloadsAreOnTheReleasePage() {
        assertEquals("https://github.com/ctrl-alt-delete-8/mi-band-bridge/releases/latest", About.DOWNLOADS)
    }

    /** Its links open the project's two licenses and its notices, in the public repository. */
    @Test
    fun aboutOpensTheLicensesAndTheNotices() {
        val repo = "https://github.com/ctrl-alt-delete-8/mi-band-bridge/blob/main"
        assertEquals(
            listOf(
                "App license" to "$repo/LICENSE-APP.md",
                "Source code license" to "$repo/LICENSE",
                "Third-party notices" to "$repo/THIRD-PARTY-NOTICES",
            ),
            About.links,
        )
    }
}
