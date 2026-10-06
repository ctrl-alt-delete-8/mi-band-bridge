#!/bin/sh
# Build both band apps with zmake.
#
# zmake converts the PNG files under assets/ into the TGA subtype the band
# reads. A plain PNG there is ignored, and the app shows no icon. Do not
# package the tree by hand for that reason.
#
# zmake is GPL-3 and is not on PyPI, so keep it outside this repository:
#
#   python3 -m venv ~/.local/share/bandbridge-tools
#   ~/.local/share/bandbridge-tools/bin/pip install Pillow qrcode
#   git clone https://github.com/melianmiko/zmake \
#       ~/.local/share/bandbridge-tools/zmake-src
set -eu

TOOLS="${BANDBRIDGE_TOOLS:-$HOME/.local/share/bandbridge-tools}"
PYTHON="$TOOLS/bin/python"
SOURCE="$TOOLS/zmake-src"

if [ ! -x "$PYTHON" ] || [ ! -d "$SOURCE" ]; then
    echo "zmake is missing. See the comment at the top of this script." >&2
    exit 1
fi

root=$(cd "$(dirname "$0")/.." && pwd)
for app in band-app band-app-arrows; do
    rm -rf "$root/$app/build" "$root/$app/dist"
    PYTHONPATH="$SOURCE" "$PYTHON" -m zmake "$root/$app" >/dev/null
    echo "built $app/dist/$app.bin"
done
