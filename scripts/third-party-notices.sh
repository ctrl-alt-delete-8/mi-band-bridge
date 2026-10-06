#!/bin/sh
# Make THIRD-PARTY-NOTICES: the licenses of what the apps include from others.
#
# The Rust crates come from cargo-about, for the Mac and Windows dependency
# sets (about.toml). The fonts, the icons, the vendored C file and the Android
# app's libraries follow, from the license files in this repository. Run it
# again after a dependency changes: for Android, compare the list below with
#   cd android && ./gradlew :app:dependencies --configuration releaseRuntimeClasspath
# cargo-about needs its command line feature:
#
#   cargo install cargo-about --locked --features cli
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
out=THIRD-PARTY-NOTICES

cargo about generate --workspace --locked --fail -c about.toml \
    scripts/third-party-notices.hbs -o "$out.partial"

part() {
    printf '\n================================================================================\n%s\n\n' "$1" >> "$out.partial"
    cat "$2" >> "$out.partial"
}

part "Roboto 2.138, Medium and Regular (the band's font, in the Mi Band App Editor's preview)
https://github.com/googlefonts/roboto-2
Apache License 2.0" editor/ui/fonts/Roboto-Apache-2.0.txt
part "Inter (the Mi Band App Editor's text)
https://github.com/rsms/inter
SIL Open Font License 1.1" editor/ui/fonts/Inter-OFL.txt
part "JetBrains Mono (the Mi Band App Editor's code and signals)
https://github.com/JetBrains/JetBrainsMono
SIL Open Font License 1.1" editor/ui/fonts/JetBrainsMono-OFL.txt
part "Lucide icons (in both apps)
https://lucide.dev
ISC License, with MIT for the icons derived from Feather" editor/ui/Lucide-LICENSE.txt
part "tiny-ECDH-c (the key exchange with the band, in Mi Band Bridge)
https://github.com/kokke/tiny-ECDH-c
The Unlicense" vendor/tiny-ecdh/LICENSE
part "Mi Band Bridge for Android: its libraries
AndroidX Core 1.13.1 (androidx.core:core-ktx), and the AndroidX libraries it
uses: annotation, annotation-experimental, arch core, collection, concurrent
futures, interpolator, lifecycle, profile installer, startup, tracing and
versioned parcelable (https://developer.android.com/jetpack/androidx). The
Kotlin standard library and kotlinx.coroutines (https://kotlinlang.org) and
JetBrains' annotations. Guava's ListenableFuture
(https://github.com/google/guava). All under the Apache License 2.0. The app
also carries the Inter and JetBrains Mono fonts and the Lucide icons, whose
licenses are above." editor/ui/fonts/Roboto-Apache-2.0.txt

# Some crates' license files end their lines in CR LF. The file keeps LF.
tr -d '\r' < "$out.partial" > "$out"
rm "$out.partial"
echo "wrote $out"
