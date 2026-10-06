# Zepp 2021 protocol notes, as measured

This file records what was confirmed against a real Xiaomi Smart Band 7 on
macOS, on 2026-09-04. `docs/REFERENCES.md` lists the sources the byte formats
were read from. The band's address and serial number here are example values.

## Transport

Both chunked characteristics live in service
`0000fee0-0000-1000-8000-00805f9b34fb`.

| Characteristic | UUID | Properties |
|---|---|---|
| Chunked write | `00000016-0000-3512-2118-0009af100700` | `WRITE_WITHOUT_RESPONSE \| NOTIFY` |
| Chunked read | `00000017-0000-3512-2118-0009af100700` | `WRITE_WITHOUT_RESPONSE \| NOTIFY` |

Neither offers plain `WRITE`. Writes are therefore always without response,
and chunk ordering depends on btleplug's write queue. This matches the
reference implementation.

`0x17` is writable. The chunk acknowledgement goes to the READ
characteristic, which looks wrong and is correct.

Negotiated MTU is 247. First-chunk payload capacity is `247 - 3 - 11 = 233`
bytes, so the 52-byte authentication message is a single chunk. The
multi-chunk splitting path is exercised only by unit tests.

## Chunk framing

First chunk, extended header, 11 bytes:

```text
[0]      0x03
[1]      flags
[2]      extended header byte, always zero so far
[3]      handle
[4]      chunk counter
[5..9]   plaintext length, u32 little-endian
[9..11]  endpoint, u16 little-endian
[11..]   payload
```

Continuation chunks carry bytes `[0..5]` only, then payload.

Flags: `0x01` first, `0x02` last, `0x04` acknowledgement requested,
`0x08` encrypted. Outgoing last chunks set `0x02 | 0x04`.

Acknowledgement bytes are `04 00 <handle> 01 <count>`, written to `0x17`.

A notification whose first byte is `0x04` is the band acknowledging us.
Ignore it.

Encrypted bodies occupy `plaintext_length + 8` bytes rounded up to a 16-byte
boundary. Decrypt with AES-128-ECB, then truncate back to the plaintext
length. The per-message key is `session_key[i] XOR handle`.

## Authentication

Endpoint `0x0082`. Confirmed on hardware:

```text
TX  04 02 00 02 <48-byte B-163 public key>
RX  10 04 01 <16-byte remote random> <48-byte remote public key>
TX  05 <AES(random, auth_key)> <AES(random, session_key)>
RX  10 05 01
```

`10 05 25` means the band rejected the key.

`session_key[i] = shared[i + 8] XOR auth_key[i]`, and the outgoing encrypted
sequence number is `shared[0..4]` little-endian.

## Post-authentication initialization is NOT needed

Section 6 of the design document left this open. It is now answered.

Mi Band Bridge sends nothing after the handshake. It does not send
`setCurrentTime`, `requestDeviceInfo`, `phase2Initialize`, or
`phase3Initialize`. Music events still arrive. Subscribing to `0x17` alone is
enough; `--subscribe-all` is not needed.

## Music endpoint

Endpoint `0x001b`. All confirmed on hardware:

| Payload | Event |
|---|---|
| `04 01` | app opened |
| `04 02` | app closed |
| `05 00` | play |
| `05 01` | pause |
| `05 03` | next |
| `05 04` | previous |
| `05 05` | volume up |
| `05 06` | volume down |

With no media playing on the Mac, the band transmits play and both volume
codes but suppresses next and previous. That is firmware behavior.

## Other observed traffic

Endpoint `0x0001` carries encrypted requests asking the host to proxy Huami
weather API calls over HTTP. Mi Band Bridge ignores them and the band retries
harmlessly. Decoding these successfully is what confirmed the encrypted
receive path against real traffic.

## Discovery behavior

The band advertises intermittently. A 6-second scan finds it about half the
time. A 20-second scan finds it reliably. Retry delays should be generous.

macOS grants Bluetooth access to a terminal without a visible prompt.
btleplug spends about 10 seconds in "Waiting on adapter connect" on every
run before the adapter is usable.

## Installing an app (measured 2026-09-05)

Install does NOT use the Zepp 2021 chunked transport. It uses the older Huami
firmware service: control on `00001531-0000-3512-2118-0009af100700`, payload on
`00001532-0000-3512-2118-0009af100700`.

Sequence. **The band drives the transfer**, and the host must follow it:

```text
-> 0xd0                        request parameters
<- 0x10 d0 01 .. <u16 LE>      band states its chunk size, at bytes [4..6]
-> 0xd2 <19 bytes>             type 0xa0, size u32, crc32 u32, chunk u16, 00 00 01, size u32
-> 0xd3 0x01                   start transfer
   one chunk on 0x1532         chunk_size bytes, written in packets of MTU-3
<- 0x10 d4 <u32 LE offset>     the band reports how much it holds, and thereby
                               asks for the chunk at that offset
   ... repeat until the reported offset reaches the file size ...
-> 0xd5                        completion, ONLY after that offset equals the size
<- 0x10 d5 01
-> 0xd6                        finalize
<- 0x10 d6 01                  installed
```

Four requirements that are easy to miss, each found by hardware failure:

- **Unsubscribe the chunked read characteristic (`0x17`) first.** Leaving it
  subscribed makes the band refuse the very first command with status `0x02`.
- **Wait for `0xd4` between chunks.** Bytes 2..6 carry the offset the next chunk
  must start at. Sending the whole payload without waiting, then announcing
  completion, is refused at `0xd5` with status `0x0b`.
- **Send `0xd5` only when the reported offset reaches the file size.** The band
  never volunteers completion, but it will refuse a premature one.
- **BLE writes are `MTU - 3` bytes.** The band reports a chunk size of 8192,
  which is a logical unit, not a packet size. Ours negotiates MTU 247, so the
  write size is 244. Conflating the two truncates the transfer 33-fold.

There is no sync command in this flow. The `0x00` byte belongs to the older
Huami protocol and Gadgetbridge does not send it for Zepp OS.

Status codes seen:

| Status | Meaning |
|---|---|
| `0x02` | the chunked read characteristic is still subscribed |
| `0x0a` | payload transferred, completion never announced |
| `0x0b` | completion announced before the band held the whole payload |
| `0x51` | the install mode in byte 12 is not supported; it must be `0x00` |
| `0x47` | not enough free space |
| `0x22` | battery too low |

`0x0b` cost a long session because it was read as a band state rather than a
protocol mistake. It is neither about the package, the app id, the version, nor
a restart: with the transfer driven by `0xd4`, two apps installed back to back
with no restart between them. Measured 2026-09-06.

### Restarting the band from the host does not work

`COMMAND_FIRMWARE_REBOOT = 0x05` to the firmware control characteristic is the
Huami restart command. harbour-amazfish and xDrip both define it, and both send
it only at the end of a firmware flash, to boot into the new firmware.

On this Band 7 it does nothing. The write is accepted, no error comes back, and
the band stays connected: measured over 60 seconds with no drop, 2026-09-06.
Restart the band by hand between installs.

### Asset sizes come from the device profile

The Zepp OS design spec gives a 124x124 app icon. That covers round watches and
is wrong for this band: the icon overflows the slot. The general spec's device
table lists 480x480, 466x466, 454x454 and 390x450 only, and never the band.

Take the size from zmake's device profile instead,
`zmake/data/zepp_devices.json`:

```json
{ "id": "mi_band7", "screenWidth": 192, "screenHeight": 490,
  "screenRadius": 96, "iconSize": 100 }
```

So the app icon is **100x100**. The band draws the file at its natural size, so
a smaller canvas sits in the corner of the slot and a larger one spills out of
it. Neither raises an error.

## Custom app signals (measured 2026-09-05)

A Zepp OS app calling `hmBle.send(buf.buffer, buf.byteLength)` surfaces on
**endpoint `0x00a0`, unencrypted, with the RAW bytes and no wrapper**:

```text
CHUNK endpoint=0x00a0 encrypted=false payload=bb01
```

Gadgetbridge's `ZeppOsAppsService` defines `CMD_JS = 0x01` with
`CMD_INCOMING = 0x00`, which suggested a `01 00` prefix. No such prefix appears.
Match the endpoint and read the payload directly.

Delivery was reliable: 15 consecutive presses, no drops.

## Writing the band app

- `zmake` leaves JavaScript untranspiled (`esbuild` is off by default). Write
  ES5: `var`, plain functions, no arrow functions, no `const` or `let`. Anything
  the band's engine cannot parse blanks the page with no error shown.
- Do not call `hmBle.createConnect` in `onInit`. A throw there leaves the page
  half drawn. Open the channel lazily on first use, inside a `try`.
- A system dialog with an `ok` button covers the app on the first launch after
  an install. It is dismissed once; later launches are clean.

## macOS code signing

Sign the binary with a developer identity. Build with `./scripts/build.sh`.

Cargo leaves a linker-signed ad-hoc signature. That signature does not register
the embedded Info.plist: `codesign -dvvv` reports no `Info.plist entries`. macOS
then ignores `NSBluetoothAlwaysUsageDescription` and kills the process the
moment it scans, with termination namespace TCC. Signing with an identity
registers the plist, and scanning works.

Signing does not make an Accessibility grant survive a rebuild. macOS pins the
code hash, so each new build needs a new grant. Measured on macOS 27.0, build
26A5425a.

## Display lists: menu, shortcuts, control center (measured 2026-09-06)

Endpoint `0x0026`, over the chunked transport, under the session key.

```text
-> 03 <type>                   request a list
<- 04 <type> <u16 LE count>    then one entry each:
                                 id, ASCII, null terminated
                                 section: 01 shown, 02 more, 03 hidden
                                 position within that section
                                 flags; bit 1 or bit 2 each add a trailing
                                 null terminated string
-> 05 <type> <u16 LE count>    replace the list, same entry layout
```

Types: `01` menu, `02` swipe shortcuts, `03` control center.

Ids are eight character upper-case hex. An installed app appears in the **menu**
as its app id in hex, for example `58E537B5`, alongside built-in screens whose
ids are small numbers.

**An installed app cannot become a swipe shortcut on this band.** Writing one
into the shortcuts list is discarded whole, leaving the list untouched. Shortcut
cards are declared in `app.json` as an `app-widget` module, and Zepp OS
documents them as needing API level 2.0. This band is 1.0, per zmake's device
profile.

Keep the flags byte and its strings when writing a list back. Rebuilding entries
without them loses whatever the band tracks there. Entries not being changed
should also keep their section, or the "more" pool collapses into "hidden".

The official app writes at most ten shortcuts.

A list may show nothing. Measured 2026-09-25 on the shortcuts: a write that
puts no entry in the main section is taken, and the band then moves the last
shown card to the front of the "more" pool, and renumbers the pool. A write
that shows the card again puts the pool back as it was.

Settings cannot be removed from the menu. The band also decides where a freshly
installed app lands. So with New app on top on, Mi Band Bridge puts an app it
has just installed first in the menu, once, on the link that comes back after
the install, and leaves every other entry where it was. It never rearranges the
menu on a connect. The command line tool's own install, and `screens --tidy`,
rebuild the menu instead: every installed app, the new one first, then settings.
An installed app is recognized by having no name in the tables above.

## Setting the clock (measured 2026-09-06)

The clock does **not** go over the chunked transport. It is written to the
standard GATT current time characteristic:

```text
00002a2b-0000-1000-8000-00805f9b34fb

<u16 LE year> <month> <day> <hour> <minute> <second>
<weekday> <fraction> <dst> <offset>
```

The band says which chunked services it has. Ask endpoint `0x0000` with `03`,
unencrypted, and it answers `04 <u16 LE count>` then `<u16 LE endpoint><u8
encrypted>` for each. Run `cargo run --example services_probe`.

This band lists 42 services and **`0x0047` is not among them**, which is why the
clock goes to the characteristic. Gadgetbridge makes the same choice in
`ZeppOsSupport.onSetTime`: the chunked time service when the band has it, the
characteristic when it does not.

The chunked time service at endpoint `0x0047` is for the other transport.
Writing there is accepted in silence and changes nothing: the band sends no
acknowledgement and the clock does not move. Gadgetbridge writes the
characteristic in `ZeppOsBtleSupport.setCurrentTime`.

- eleven bytes, with no command byte
- the month counts from one, and the weekday from zero on **Sunday**
- `dst` is `0x08` while summer time is in effect, `0x00` otherwise. Sending
  zero the year round leaves the band unmoved
- `fraction` is the part of a second in 1/256ths
- `offset` is the signed offset from UTC in quarter hours

The band keeps time well enough by itself, so this is sent once per connect.
A face showing an old time is a face that did not redraw: Zepp OS pauses a
watch face while the screen is off, and one that redraws only on MINUTEEND
shows the frame from before it slept.

Zepp OS reports no screen event. Gadgetbridge's Huami device events are
`FELL_ASLEEP`, `WOKE_UP`, `STEPSGOAL_REACHED`, `BUTTON_PRESSED`, `START_NONWEAR`,
`CALL_REJECT` and `MUSIC_CONTROL`, with nothing for the display, and none of
them are wired up in its Zepp OS path. `SCREEN_ON_ON_NOTIFICATIONS` and
`LIFT_WRIST_MODE` are settings sent **to** the band, not reports from it.

The closest signal is the band's own traffic on endpoint `0x0001`: it asks for
weather and the like when it is awake and being looked at. The clock is set
then, and every five minutes besides.

The characteristic reads back, which is the way to tell a clock fault from a
display fault: `cargo run --example clock_probe` reads it, writes it, and reads
it again. The band keeps the offset and normalizes the daylight saving byte to
zero.

The band has no clock it can trust on its own. It takes the time from whichever
host it is paired with, and drifts between those moments. While this program
holds the band the phone cannot reach it, so nothing else sets it and the watch
face falls minutes behind. This is sent on every connect.

## macOS input permission, measured

macOS grants the input permission to the **responsible process**, not to this
binary. When a shell inside another program starts Mi Band Bridge, that program
is the responsible process, and its permission decides the answer.

Read the attribution from the system log. Use the absolute path, because `log`
is also a zsh builtin:

    /usr/bin/log stream --style compact \
      --predicate '(process BEGINSWITH "tcc") AND (eventMessage CONTAINS "bandbridge")'

A denied start prints:

    AUTHREQ_ATTRIBUTION: attribution={
      responsible={identifier=<the program that started it>, ...},
      accessing={identifier=com.ellengu.bandbridge, ...},
      requesting={identifier=com.ellengu.bandbridge, ...} }

`responsible` names the entry that must be switched on. Adding this binary, or
an app bundle around it, changes nothing while another program is responsible.

To check the answer without starting Mi Band Bridge, build a two-line probe:

    import ApplicationServices
    print(AXIsProcessTrusted())

Run it from the same shell. It reports the same answer, and it starts no
Bluetooth work.

## The standard Bluetooth services

Beside its own chunked protocol the band carries the ordinary GATT services,
and two of them answer questions the chunked side cannot. Plain reads, no
authentication. Run `cargo run --example device_probe`.

| Characteristic | What the band answered |
|---|---|
| `2a23` System ID | `00005efffe005301` |
| `2a25` serial number | `12345/ABCDE12XY34567` |
| `2a27` hardware revision | `0.91.130.5` |
| `2a28` software revision | `2.2.0.1`, the firmware |
| `2a19` battery level | `38`, which is 56 percent |

**The address is in the System ID.** macOS never gives an app a Bluetooth
address: CoreBluetooth hands out a UUID of its own, and reports the address as
zeros. The band fills its System ID the conventional way, three address bytes
of the maker, then `FF FE`, then three bytes of the device, so
`00005e fffe 005301` is `00:00:5E:00:53:01`. The band's advertised name ends in
`5301`, which is the same last two bytes.

`2a19` is a second way to the battery level, and a simpler one, but it carries
no charging state. The chunked service below carries both.

## Battery, endpoint `0x0029`

Ask with one byte, `03`. The band answers `04`, then one byte nothing here
reads, then the level, then the state, where `01` is charging. The rest of the
21 bytes is the last charge time and the level it reached. Measured on the
band:

```text
CHUNK endpoint=0x0029 encrypted=false payload=040f3800ea070907000c12f0ea070907000c12f064
                                              ^^      04 = a battery reply
                                                ^^    not read
                                                  ^^  0x38 = 56%
                                                    ^^ 00 = not charging
```

**Send it unencrypted.** Gadgetbridge builds this service encrypted and then
overrides that from the service list, and this band lists `0x0029 encrypted=0`.
An encrypted request is ignored in silence: no answer, no error. That silence
cost an hour, so read the `encrypted` flag in the service list before deciding.

The band also sends this reply on its own when the level changes, so a host
that asks once keeps hearing about it.

## Config service, endpoint `0x000a`

The band keeps its settings here, encrypted. Gadgetbridge calls this
`ZeppOsConfigService`. Run `cargo run --example config_probe` to read every
group, and `cargo run --example screen_timeout` to read or set the screen-on
time.

Settings sit in groups. Ask for one:

```text
03 <constraints> <group> <arg count> [<arg code> ...]
```

`constraints` of `01` makes the band report the allowed values with each
setting. An arg count of zero asks for the whole group. The reply is:

```text
04 01 <group> <version> <constraints> <count> [<arg code> <type> <value> ...]
```

A write is `05 <group> <version> 00 <count> [<arg code> <type> <value> ...]`,
answered with `06 <status>`. Status `0x02` means refused.

Types: `0x01` short, `0x02` short list, `0x03` int, `0x0b` bool, `0x10` byte,
`0x11` byte list, `0x20` string, `0x21` string list, `0x30` hh:mm, `0x40`
timestamp, `0x50` unbound int. With constraints on, a byte carries its value,
a count, then that many allowed values. A short carries value, minimum, and
maximum. A bool carries a value alone.

### Which groups this band has

Measured: `0x00` agps, `0x01` display, `0x03` sound, `0x04` lockscreen, `0x07`
language, `0x08` health, `0x09` workout, `0x0a` system, `0x0b` bluetooth.

Groups `0x02`, `0x05` wearing direction, and `0x06` offline voice answer
`04 03`, which is a refusal. Gadgetbridge marks `0x02` as unidentified. This
band does not have it either.

### The screen-on time is capped at 15 seconds

`screen_timeout` is byte `0x03` of the display group. The band reports the
value in seconds and declares its allowed set:

```text
screen timeout is 15 seconds
the band says it allows: 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15
```

Writing 30 is refused with status `0x02`, and a read back still says 15. The
cap is in the firmware, not in the band's menu. Raising the screen-on time
across the whole band is therefore not possible from here.

A band app can still hold its own screen lit with
`hmSetting.setBrightScreen(seconds)`, which is what the two apps in this
project do. That works while the band is worn. The band turns the screen off
when it decides it is off the wrist, and nothing in this service changes that.

### Three settings Gadgetbridge does not name

The system group reports 17 settings. Gadgetbridge names 14 of them. These
three are not in its table:

| Arg | Type | Value here | Tried |
|---|---|---|---|
| `0x09` | bool | 1 | set to 0, accepted, changed nothing anyone noticed |
| `0x19` | bool | 1 | set to 0, accepted, changed nothing anyone noticed |
| `0x1f` | bool | 0 | not tried |

Both writes were accepted with status `0x01` and read back changed, so the
service takes them. Neither stopped the band turning its screen off when it is
not on a wrist, and neither broke anything visible. Both are back at their
original values. Use `cargo run --example config_set` to try `0x1f`.

The off-wrist screen off is not a setting on this band. Nothing in this
service names it, and no device app API overrides it. Always-on display is a
watch face feature: it shows the watch face, not an app.
