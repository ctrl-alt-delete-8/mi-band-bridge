#!/usr/bin/env bash
# Shoot one screen of the relay on the emulator, seeded with a debug fixture.
#
#   tools/shoot.sh [--no-build] <fixture> <out.png> [phone|tablet]
#
# Builds the debug APK (unless --no-build), installs it on emulator-5554, sets
# the emulator to a phone (1080x2400 at 420 dpi, 412 x 914 dp) or a tablet
# (2560x1600 at 320 dpi, 1280 x 800 dp), clears the app's data, grants the
# notification permission so no dialog covers the shot, launches the app with
# the fixture, waits 2 s, and saves a screenshot to <out.png>.
#
# Fixtures (src/debug/.../Fixtures.kt): working, empty, gate, gate-keymapper,
# long, test-lost, kit, menu, imported. The emulator keeps the size
# afterwards, so taps and a screencap can follow. This script only ever talks
# to emulator-5554, and refuses to run when it is not attached: a Sony reader
# may be on USB too.
set -euo pipefail

readonly DEVICE="emulator-5554"
readonly PACKAGE="dev.bandbridge.relay"

usage() {
    echo "usage: $0 [--no-build] <fixture> <out.png> [phone|tablet]" >&2
    exit 2
}

build=1
args=()
for arg in "$@"; do
    case "$arg" in
        --no-build) build=0 ;;
        -h | --help) usage ;;
        -*) echo "shoot.sh: unknown option $arg" >&2; usage ;;
        *) args+=("$arg") ;;
    esac
done
[[ ${#args[@]} -ge 2 && ${#args[@]} -le 3 ]] || usage
fixture="${args[0]}"
out="${args[1]}"
form="${args[2]:-phone}"
case "$form" in
    phone) size="1080x2400"; density="420" ;;
    tablet) size="2560x1600"; density="320" ;;
    *) echo "shoot.sh: the form is phone or tablet, not $form" >&2; usage ;;
esac

android_dir="$(cd "$(dirname "$0")/.." && pwd)"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
if command -v adb >/dev/null 2>&1; then
    adb_bin="adb"
else
    adb_bin="$ANDROID_HOME/platform-tools/adb"
fi

# The one check before anything else: no build, no install, no shell on any
# device until emulator-5554 is attached and ready.
if ! "$adb_bin" devices | awk -v want="$DEVICE" 'NR > 1 && $1 == want && $2 == "device" { found = 1 } END { exit !found }'; then
    echo "shoot.sh: $DEVICE is not attached and ready in 'adb devices'. Start the emulator first. No other device is used." >&2
    exit 1
fi

emu() {
    "$adb_bin" -s "$DEVICE" "$@"
}

mkdir -p "$(dirname "$out")"
out="$(cd "$(dirname "$out")" && pwd)/$(basename "$out")"

if [[ $build -eq 1 ]]; then
    (cd "$android_dir" && ./gradlew --quiet assembleDebug)
fi
emu install -r "$android_dir/app/build/outputs/apk/debug/app-debug.apk" >/dev/null

emu shell wm size "$size"
emu shell wm density "$density"
emu shell pm clear "$PACKAGE" >/dev/null
# Android 13 and later ask for this permission. Older ones have no such
# permission, and nothing to grant.
if [[ "$(emu shell getprop ro.build.version.sdk | tr -d '\r')" -ge 33 ]]; then
    emu shell pm grant "$PACKAGE" android.permission.POST_NOTIFICATIONS
fi
# The launcher's action and category, so that Home and a reopen from the
# launcher bring this window back, rather than start a second one that has
# no fixture.
emu shell am start -W -a android.intent.action.MAIN -c android.intent.category.LAUNCHER \
    -n "$PACKAGE/.MainActivity" --es fixture "$fixture" >/dev/null
sleep 2
emu exec-out screencap -p >"$out"
echo "$out ($form, fixture $fixture)"
