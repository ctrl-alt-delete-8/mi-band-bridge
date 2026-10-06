package dev.bandbridge.relay

/**
 * The Setup page's About section: the app's name and version, what it is
 * not, and its two licenses and its notices, which open in the browser. Pure,
 * so it is tested without a device. The desktop apps' About says the same.
 */
object About {
    /** The project's public repository, which holds the licenses and the notices. */
    private const val REPOSITORY = "https://github.com/ctrl-alt-delete-8/mi-band-bridge"

    /**
     * The release page, with the desktop app and the sample band app, which
     * Setup's notice opens. The user's address, exactly.
     */
    const val DOWNLOADS = "$REPOSITORY/releases/latest"

    const val UNOFFICIAL = "Unofficial. Not made by or connected with Xiaomi."

    const val LICENSES = "The app is free to use, at work too. Its source code is under PolyForm Noncommercial 1.0.0."

    /** The first line: the app's name, and its version when the phone says it. */
    fun name(version: String?): String =
        if (version.isNullOrBlank()) "Mi Band Bridge" else "Mi Band Bridge $version"

    /** The pages the section opens, each with its button's label, in the order they show. */
    val links: List<Pair<String, String>> = listOf(
        "App license" to "$REPOSITORY/blob/main/LICENSE-APP.md",
        "Source code license" to "$REPOSITORY/blob/main/LICENSE",
        "Third-party notices" to "$REPOSITORY/blob/main/THIRD-PARTY-NOTICES",
    )
}
