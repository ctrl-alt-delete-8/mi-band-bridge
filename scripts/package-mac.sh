#!/bin/sh
# Make the Mac release: dist/mac/Mi-Band-Bridge-<version>-mac-arm64.dmg, with
# Mi Band Bridge and the Mi Band App Editor in it.
#
# Both apps are built for Apple Silicon (arm64) in release mode, signed with
# the Developer ID and the hardened runtime, notarized by Apple, and stapled.
# The disk image is signed, notarized and stapled too. Then the script checks
# each, as Gatekeeper sees a download: a quarantined copy must pass.
#
# Notarization goes through notarytool's keychain profile, named here. The
# script names only the profile, and never reads or prints its password. Make
# the profile once with:
#   xcrun notarytool store-credentials mi_band_mac_app --apple-id <you> --team-id CYS5988KRM
#
# Run it from a checkout at a neutral path, such as /private/tmp/mbb-release.
# The build maps this Mac's paths out of the binaries, but a path the compiler
# does not see, such as one a build step writes as text, stays as it is.
# Before signing, scripts/scan-mac-binaries.py scans the binaries for private
# details, and a hit stops the release.
#
# It writes nothing outside target/ and dist/, and it installs nothing.
#
#   ./scripts/package-mac.sh
set -eu

# codesign takes any part of a certificate's name that names one certificate.
# With one Developer ID Application certificate in the Keychain, this default
# finds it. With several, set MAC_RELEASE_IDENTITY to the full name.
IDENTITY="${MAC_RELEASE_IDENTITY:-Developer ID Application}"
PROFILE="${MAC_NOTARY_PROFILE:-mi_band_mac_app}"

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

if [ "$(uname -m)" != arm64 ]; then
    echo "Build the Mac release on an Apple Silicon Mac. This one is $(uname -m)." >&2
    exit 1
fi

# A build writes the paths of this Mac into the binaries, through panic
# messages and file!(): the cargo registry's sources, the toolchain's, and
# this checkout's. Each is mapped to a neutral name, from the environment, so
# this script names no person. The home folder goes first, as a fallback:
# rustc uses the last mapping that matches. CARGO_ENCODED_RUSTFLAGS takes paths
# with spaces, which RUSTFLAGS cannot, and it replaces RUSTFLAGS for the build.
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
rustup_home="${RUSTUP_HOME:-$HOME/.rustup}"
sep=$(printf '\037')
CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$HOME=/home$sep--remap-path-prefix=$cargo_home=/cargo$sep--remap-path-prefix=$rustup_home=/rustup$sep--remap-path-prefix=$root=/build"
export CARGO_ENCODED_RUSTFLAGS

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
bridge="Mi Band Bridge.app"
editor="Mi Band App Editor.app"
work="target/mac-release"
out="dist/mac"
dmg="$out/Mi-Band-Bridge-$version-mac-arm64.dmg"
notes="$out/notarization.txt"

rm -rf "$work"
mkdir -p "$work/dmg" "$out"
rm -f "$dmg" "$dmg.sha256" "$notes"

echo "== Build $version for arm64, in release mode"
# The bridge's bundle comes from build.sh, which keeps its own Info.plist and
# bundle identifier. It is signed ad hoc there, and again here.
BANDBRIDGE_SIGN_IDENTITY=- ./scripts/build.sh --release
ditto "target/$bridge" "$work/$bridge"

# The editor's bundle comes from Tauri's bundler, pinned to the Tauri the
# apps build with, as on Windows.
cli="target/tauri-cli/bin/cargo-tauri"
if [ ! -x "$cli" ]; then
    echo "Building the Tauri 2 command line tool into target/tauri-cli..."
    cargo install tauri-cli --version '~2.11' --locked --root target/tauri-cli
fi
(cd editor && "../$cli" build --bundles app)
ditto "target/release/bundle/macos/$editor" "$work/$editor"

for app in "$bridge" "$editor"; do
    for binary in "$work/$app/Contents/MacOS/"*; do
        archs=$(lipo -archs "$binary")
        if [ "$archs" != arm64 ]; then
            echo "$binary is for $archs, not arm64 alone." >&2
            exit 1
        fi
    done
done

echo "== Scan the binaries for private details"
# Before signing, because a Developer ID signature carries its holder's name by
# design. scan-mac-binaries.py holds the patterns, and a hit stops the release.
python3 scripts/scan-mac-binaries.py "$work/$bridge" "$work/$editor"

echo "== Sign with $IDENTITY and the hardened runtime"
# The bridge needs one entitlement: an action's script may send Apple events,
# and the app answers for its scripts (packaging/bridge.entitlements). The
# editor needs none.
codesign --force --options runtime --timestamp \
    --entitlements packaging/bridge.entitlements \
    --sign "$IDENTITY" "$work/$bridge"
codesign --force --options runtime --timestamp --sign "$IDENTITY" "$work/$editor"
for app in "$bridge" "$editor"; do
    codesign --verify --deep --strict --verbose=2 "$work/$app"
done

# Send one file to Apple, wait for the answer, and write the submission's id
# and status to the notes. notarytool exits 0 once Apple answers, even with a
# rejection, so the answer's status decides: a rejection prints Apple's log and
# stops here. Another exit is a failure to submit, such as a network or sign-in
# error, and stops here with notarytool's message.
notarize() {
    label=$1
    file=$2
    answer="$work/$label.notarytool.json"
    if ! xcrun notarytool submit "$file" --keychain-profile "$PROFILE" --wait \
        --output-format json > "$answer"; then
        echo "notarytool could not submit $label:" >&2
        cat "$answer" >&2
        exit 1
    fi
    id=$(plutil -extract id raw -o - "$answer")
    status=$(plutil -extract status raw -o - "$answer")
    echo "$label: $id $status" | tee -a "$notes"
    if [ "$status" != Accepted ]; then
        xcrun notarytool log "$id" --keychain-profile "$PROFILE" >&2
        exit 1
    fi
}

echo "== Notarize and staple each app"
for app in "$bridge" "$editor"; do
    zip="$work/${app%.app}.zip"
    ditto -c -k --keepParent "$work/$app" "$zip"
    notarize "$app" "$zip"
    xcrun stapler staple "$work/$app"
    xcrun stapler validate "$work/$app"
done

echo "== Make, sign, notarize and staple the disk image"
ditto "$work/$bridge" "$work/dmg/$bridge"
ditto "$work/$editor" "$work/dmg/$editor"
ln -s /Applications "$work/dmg/Applications"
hdiutil create -volname "Mi Band Bridge $version" -srcfolder "$work/dmg" \
    -fs HFS+ -format UDZO -ov "$dmg"
codesign --force --timestamp --sign "$IDENTITY" "$dmg"
notarize "$(basename "$dmg")" "$dmg"
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"

echo "== Check a quarantined copy, as a download is"
check="$work/check"
mkdir -p "$check"
quarantine="0081;$(printf '%x' "$(date +%s)");Safari;"
cp "$dmg" "$check/download.dmg"
xattr -w com.apple.quarantine "$quarantine" "$check/download.dmg"
spctl --assess --type open --context context:primary-signature -vv "$check/download.dmg"
mount="$check/mount"
mkdir -p "$mount"
hdiutil attach -nobrowse -readonly -mountpoint "$mount" "$check/download.dmg" > /dev/null
# A failed check still leaves no disk mounted.
trap 'hdiutil detach "$mount" > /dev/null 2>&1 || true' EXIT
for app in "$bridge" "$editor"; do
    ditto "$mount/$app" "$check/$app"
    xattr -w com.apple.quarantine "$quarantine" "$check/$app"
    spctl --assess --type execute -vv "$check/$app"
    xcrun stapler validate "$check/$app"
done
hdiutil detach "$mount" > /dev/null
trap - EXIT

# By the file's name alone, so `shasum -c` checks it beside a download.
(cd "$out" && shasum -a 256 "$(basename "$dmg")") | tee "$dmg.sha256"
echo "made $dmg"
