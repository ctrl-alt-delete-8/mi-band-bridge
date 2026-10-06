#!/bin/sh
# Put the built app where the system looks for applications.
#
# Raycast, Spotlight, and the Finder search /Applications and ~/Applications.
# The build writes to target/, which none of them index.
#
# macOS ties the input permission to the code signature and its path, so the
# copy needs its own grant the first time. Grant it once and it holds.
#
# Version 1.0.0 renamed BandBridge.app to "Mi Band Bridge.app". Both are the
# same app, and two copies would both try to hold the band, so the old one
# goes when the new one comes. Its settings and the band's key stay: they are
# in ~/Library/Application Support/BandBridge and in the Keychain, not in the
# app.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
name="Mi Band Bridge.app"
built="$root/target/$name"
old_name="BandBridge.app"

if [ ! -d "$built" ]; then
    echo "no build at $built. Run ./scripts/build.sh first." >&2
    exit 1
fi

if [ -w /Applications ]; then
    dest=/Applications
else
    dest="$HOME/Applications"
    mkdir -p "$dest"
fi

# A running copy cannot be replaced cleanly, and an old one must not keep
# holding the band.
for app in "$dest/$name" "/Applications/$old_name" "$HOME/Applications/$old_name"; do
    pkill -f "$app/Contents/MacOS/bandbridge-gui" 2>/dev/null || true
done
sleep 1

for old in "/Applications/$old_name" "$HOME/Applications/$old_name"; do
    if [ -d "$old" ]; then
        if rm -rf "$old"; then
            echo "removed the old $old"
        else
            echo "could not remove the old $old. Remove it by hand, so it cannot hold the band." >&2
        fi
    fi
done

rm -rf "$dest/$name"
cp -R "$built" "$dest/$name"
echo "installed $dest/$name"
echo "  open it: open -a \"$dest/$name\""
