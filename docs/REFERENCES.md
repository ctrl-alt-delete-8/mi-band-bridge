# References

## Protocol reference

Gadgetbridge, AGPLv3, https://codeberg.org/Freeyourgadget/Gadgetbridge

Used as a protocol reference only. The byte formats in `PROTOCOL.md` were
read from these files and reimplemented in Rust:

- `service/devices/huami/Huami2021ChunkedDecoder.java`
- `service/devices/huami/Huami2021ChunkedEncoder.java`
- `service/devices/huami/operations/init/InitOperation2021.java`
- `service/devices/huami/zeppos/services/ZeppOsMusicService.java`
- `devices/huami/HuamiService.java`

## Vendored code

tiny-ECDH-c, Unlicense, https://github.com/kokke/tiny-ECDH-c
Local copy in `vendor/tiny-ecdh`. See `vendor/tiny-ecdh/PATCHES.md`.

## Library reference

btleplug 0.13, https://github.com/deviceplug/btleplug

## License

The source code is under the PolyForm Noncommercial License 1.0.0, in
`LICENSE`. The apps, as this project publishes them, are under
`LICENSE-APP.md`: free for any use, at work too, and free to share unchanged,
but not to sell. `THIRD-PARTY-NOTICES` gives the licenses of what the apps
include from others. `scripts/third-party-notices.sh` makes it again.

The protocol was reimplemented from documented byte formats rather than
copied from Gadgetbridge, which is AGPLv3. Copying Gadgetbridge source would
force AGPLv3 on any published derivative, so do not.

## Band app editor

tiny-skia 0.12, BSD-3-Clause, https://github.com/linebender/tiny-skia
Draws the app icon and button images.

Roboto 2.138 Medium and Regular, Apache-2.0,
https://github.com/googlefonts/roboto-2
The band's own font. The editor measures labels with Medium, and the preview
draws them in Regular. The files are `Roboto-Medium.ttf` and
`Roboto-Regular.ttf` from `roboto-android.zip` of the v2.138 release. Their
license is in `editor/ui/fonts/Roboto-Apache-2.0.txt`.

ttf-parser 0.25, MIT or Apache-2.0, https://github.com/harfbuzz/ttf-parser
Reads Roboto's advance widths and line height, to measure labels.

Tauri 2, MIT or Apache-2.0, https://tauri.app

zmake, GPL-3.0, https://github.com/melianmiko/zmake
The editor runs zmake as a separate program. zmake is not included in this
repository or in the editor.
