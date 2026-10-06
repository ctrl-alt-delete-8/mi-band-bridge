#!/bin/sh
# Build Mi Band Bridge and sign it with a stable identity.
#
# macOS 26 and later will not hold an Accessibility grant for a bare executable.
# The entry appears in the settings list and reads as on, but the system ignores
# it. Apple confirms the bug in developer forums thread 808897. It affects other
# privileges in the same pane. So wrap the window in an .app bundle, which the
# system does handle, and grant the bundle.
#
# The result is "target/Mi Band Bridge.app". scripts/install-app.sh copies it
# to /Applications.
set -eu

# Pick the signing identity. The first one that is set wins:
#   1. BANDBRIDGE_SIGN_IDENTITY in the environment
#   2. the first line of ~/.config/bandbridge/sign-identity
#   3. "-", an ad hoc signature, which needs no Apple account
#
# macOS records a permission against the signature. An ad hoc signature names a
# hash of the binary, so every rebuild looks like a new app, and macOS asks for
# Bluetooth and input permission again. A certificate names the same identity on
# every build, so a grant survives a rebuild. To list your identities, run:
#   security find-identity -v -p codesigning
identity_file="$HOME/.config/bandbridge/sign-identity"
if [ -n "${BANDBRIDGE_SIGN_IDENTITY:-}" ]; then
    IDENTITY="$BANDBRIDGE_SIGN_IDENTITY"
elif [ -s "$identity_file" ]; then
    IDENTITY="$(head -n 1 "$identity_file")"
else
    IDENTITY="-"
fi
BUNDLE_ID="com.ellengu.bandbridge"

cargo build "$@"
cargo build -p bandbridge-gui "$@"

profile=debug
for arg in "$@"; do
    [ "$arg" = "--release" ] && profile=release
done

# The bundle holds the window alone, because the window is the only entry
# point (design decision D9).
# The first cargo build also makes the command line tool, which is for protocol
# probes. It stays at target/$profile/bandbridge and never goes in the bundle:
# see docs/DEVELOPER.md. It never shared the window's code identity either:
# inside the bundle it kept the linker's ad hoc signature.
app="target/Mi Band Bridge.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "target/$profile/bandbridge-gui" "$app/Contents/MacOS/bandbridge-gui"
cp gui/icons/icon.png "$app/Contents/Resources/icon.png"
# The licenses and the notices of what the app includes from others.
cp LICENSE LICENSE-APP.md THIRD-PARTY-NOTICES "$app/Contents/Resources/"
sed -e "s|@IDENTIFIER@|$BUNDLE_ID|" packaging/Info.plist > "$app/Contents/Info.plist"
codesign -f -s "$IDENTITY" "$app"
echo "built $app"
if [ "$IDENTITY" = "-" ]; then
    echo "  signed ad hoc: macOS asks for permissions again after each rebuild."
    echo "  To keep them, put a signing identity in $identity_file"
else
    echo "  signed as: $IDENTITY"
fi
echo "  window: $app/Contents/MacOS/bandbridge-gui"
