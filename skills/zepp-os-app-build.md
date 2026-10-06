
# Building a Zepp OS app

This covers a **device app**, the kind that appears in the band's app list. A
**watch face** uses a different toolchain and will render black if built this
way.

Facts here are measured on a Xiaomi Smart Band 7 running Zepp OS 1.0.

## The toolchain

Package with **zmake**. It is GPL-3 and is not on PyPI, so keep it outside the
project:

```sh
python3 -m venv ~/.local/share/bandbridge-tools
~/.local/share/bandbridge-tools/bin/pip install Pillow qrcode
git clone https://github.com/melianmiko/zmake ~/.local/share/bandbridge-tools/zmake-src
PYTHONPATH=~/.local/share/bandbridge-tools/zmake-src \
  ~/.local/share/bandbridge-tools/bin/python -m zmake path/to/app
```

Do not hand-zip the tree. zmake adds `packageInfo` and `platforms` to
`app.json`, and it converts the assets, which is the part that matters.

## Assets are TGA, not PNG

Files under `assets/` must be a **TGA subtype**, not PNG. The band ignores a
PNG there: no icon, no error, no clue. zmake converts PNG to TGA-P by default,
and the filename suffix picks another: `.p.png`, `.rlp.png`, `.rgb.png`,
`.rgba.png`.

## Take sizes from the device profile

The Zepp OS design guide gives a 124 by 124 app icon. That covers round
watches. Its device table lists 480x480, 466x466, 454x454 and 390x450, and
never a band.

Read the real numbers from `zmake/data/zepp_devices.json`:

```json
{ "id": "mi_band7", "screenWidth": 192, "screenHeight": 490,
  "screenRadius": 96, "iconSize": 100 }
```

So the icon is **100 by 100**. The band draws an asset at its natural size, so
a smaller one sits in the corner of its slot and a larger one spills out of it.
Neither raises an error.

## Write the page in ES5

zmake does not transpile: `esbuild` is off by default. Use `var`, plain
functions, and string concatenation. No arrow functions, no `const` or `let`,
no template literals. Anything the engine cannot parse blanks the page with no
error shown.

A blank page after a change is nearly always a parse error or a throw during
`onInit`.

## Open BLE lazily

Do not call `hmBle.createConnect` in `onInit`. A throw there leaves the page
half built, which looks the same as a blank screen. Open the channel on first
use, inside a `try`.

## Layout

- A `BUTTON` with artwork takes `normal_src` and `press_src` **together**. One
  with no artwork takes `normal_color` and `press_color` instead.
- `normal_src` is the button's background, so the artwork must be the button's
  own size. A smaller image does not stretch.
- Button text renders centered. Put artwork in the upper half if a label must
  sit below it.
- Leave a strip of bare screen at the bottom, 70px, so a back swipe has
  somewhere to start that is not a button. Under 30px, a swipe may press a
  button.

## Signals to a host

`hmBle.send(buf.buffer, buf.byteLength)` reaches the host on endpoint `0x00a0`
with the **raw bytes and no wrapper**. Gadgetbridge's `CMD_JS = 0x01` prefix
does not appear. Match the endpoint and read the payload directly.

The band never tells a host which app sent a signal, and it hands back nothing
but an app id for what is installed. So an app declares what it sends, in a
`signals.json` beside its `app.json`, and the tools read that file.

## Installing without a phone

Install uses the older Huami firmware service, not the chunked transport:
control `00001531-...`, payload `00001532-...`.

```text
-> 0xd0                        request parameters
<- 0x10 d0 01 .. <u16 LE>      the band's chunk size, at bytes [4..6]
-> 0xd2 <19 bytes>             type 0xa0, size u32, crc32 u32, chunk u16, 00 00 01, size u32
-> 0xd3 0x01                   start transfer
   one chunk on 0x1532         in packets of MTU-3
<- 0x10 d4 <u32 LE offset>     how much the band holds, and a request for the rest
   ... repeat until that offset reaches the file size ...
-> 0xd5                        completion, ONLY then
-> 0xd6                        finalize
```

Four rules, each found by hardware failure:

- **Unsubscribe the chunked read characteristic first.** Leaving it subscribed
  makes the band refuse the first command with `0x02`.
- **Let the band drive.** Bytes 2..6 of `0xd4` are the offset the next chunk
  starts at. Sending everything and then announcing completion is refused.
- **Send `0xd5` only when that offset reaches the file size.**
- **Writes are `MTU - 3` bytes.** The band's chunk size of 8192 is a logical
  unit, not a packet size.

There is no sync command in this flow. The `0x00` byte belongs to the older
Huami protocol.

### Status bytes

| Status | Meaning |
|---|---|
| `0x02` | the chunked read characteristic is still subscribed |
| `0x0a` | payload transferred, completion never announced |
| `0x0b` | completion announced before the band held the whole payload |
| `0x51` | the install mode in byte 12 is not supported. It must be `0x00` |
| `0x47` | not enough free space |
| `0x22` | battery too low |

`0x0b` is not about the package. Do not repackage in response to it: the same
file that is refused installs cleanly once the transfer is driven correctly.

## After an install

The band puts a new app in the menu wherever it likes. With New app on top on,
Mi Band Bridge moves the new app to the top of the menu, once, after the
install. It never rearranges the menu on a connect, so an order the person set
stays. `docs/PROTOCOL.md` has the menu's format.

## What the band does not report

The app list reply carries `<appIdHex>-<version>` pairs only. **No name.** A
host cannot show an app's name from the band, which is why Gadgetbridge shows
`unknown (xxxx)`. Generate a name table from your own `app.json` files instead.
