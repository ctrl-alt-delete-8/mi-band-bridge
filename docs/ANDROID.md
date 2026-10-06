# The same band buttons on Android

On macOS this project holds the band itself and posts a keystroke. On Android
you do not need to hold the band, and you cannot post a keystroke. Gadgetbridge
already holds the band, and Android's way of acting on an event is an intent.

So the shape is different:

```text
macOS    band -> Mi Band Bridge -> CGEvent keystroke -> the focused app
Android  band -> Gadgetbridge -> broadcast intent -> Tasker, or your own app
```

Nothing has to be written in Rust, and nothing has to talk BLE. Gadgetbridge
hands you the same bytes this project decodes.

## Does it work on this band?

Yes. Gadgetbridge's BLE intent API lives in `BleIntentApi`, which is built by
`AbstractBTLESingleDeviceSupport`. The Xiaomi Smart Band 7 runs through
`ZeppOsBtleSupport`, and that class extends it:

```java
public class ZeppOsBtleSupport extends AbstractBTLESingleDeviceSupport
        implements ZeppOsCommunicator
```

This is worth stating, because `ZeppOsSupport`, the class that holds the Zepp
protocol, extends something else entirely. The transport class is the one that
carries the intent API, and the band uses it.

## Which Gadgetbridge you need

| Version | What it added |
|---|---|
| 0.82.0 | the BLE intent API |
| 0.84.0 | the characteristic filter |
| 0.94.0 | more than one target package, comma separated, each one trimmed |

0.84.0 or newer is the one to want, and 0.94.0 or newer is safe from **The trap
that cost an evening**. The API broadcasts raw characteristic bytes and does
not interpret them, so Gadgetbridge needs no feature for Zepp OS custom app
signals. It does not have one, and that does not matter here.

Below 0.82.0 there is no such API, and an app of your own is the way.

## Switch it on

Per device, in Gadgetbridge: the device's settings, then Developer options,
then BLE Intent API. The preference keys are:

| Key | What it does |
|---|---|
| `prefs_device_ble_api_characteristic_notify` | broadcast every notification |
| `prefs_device_ble_api_characteristic_read_write` | accept read and write commands |
| `prefs_device_ble_api_state` | broadcast connect and disconnect |
| `prefs_device_ble_api_filter_char` | only these characteristic UUIDs |
| `prefs_device_ble_api_package` | send to these packages, comma separated |

Set the filter to the chunked read characteristic, or every band notification
arrives and you will be matching against a flood:

```text
00000017-0000-3512-2118-0009af100700
```

Set the package to your automation app, for the reason in **One real trap**
below.

## The intent

```text
action  nodomain.freeyourgadget.gadgetbridge.ble_api.events.CHARACTERISTIC_CHANGED
extras  EXTRA_DEVICE_ADDRESS   the band's MAC address
        EXTRA_CHARACTERISTIC   the characteristic UUID
        EXTRA_PAYLOAD          the bytes, hex, upper case, no separators
```

The extra holding the UUID is named `EXTRA_CHARACTERISTIC` on the way out and
`EXTRA_CHARACTERISTIC_UUID` on the way in. That is Gadgetbridge's naming, not a
mistake in this document. Use the right one for the direction you are going.

## What a button press looks like

A press of the BandBridge band app sends `bb01` on endpoint `0x00a0`,
unencrypted, so the whole chunk arrives in clear. The framing is in
`docs/PROTOCOL.md`:

```text
03 03 00 HH 00  02 00 00 00  A0 00  BB 01
|  |  |  |  |   |            |      |
|  |  |  |  |   |            |      the signal
|  |  |  |  |   |            endpoint 0x00a0, little endian
|  |  |  |  |   payload length, 2, u32 little endian
|  |  |  |  chunk counter
|  |  |  handle, changes between messages
|  |  extended header
|  flags: first and last chunk
chunk marker
```

`EXTRA_PAYLOAD` is therefore `030300HH0002000000A000BB01`, with `HH` differing
from press to press. So do not match the whole string. **Match the last four
bytes:**

| App | Button | Match `EXTRA_PAYLOAD` ends with |
|---|---|---|
| BandBridge | the one button | `A000BB01` |
| BandArrows | up | `A000BB02` |
| BandArrows | down | `A000BB03` |

Those are the endpoint and the signal together, which no other traffic carries.

## With Tasker

1. Profile, Event, System, Intent Received.
2. Action:
   `nodomain.freeyourgadget.gadgetbridge.ble_api.events.CHARACTERISTIC_CHANGED`
3. Task, first action If: `%EXTRA_PAYLOAD` matches `*A000BB01`
4. Then whatever you want: launch an app, send a media key, run a shell
   command, toggle a setting.

Put `net.dinglisch.android.taskerm` in `prefs_device_ble_api_package`.

### The two actions worth wiring

**A media key**, which is what `bb02` and `bb03` do on macOS: Tasker's Media
Control action, with Cmd set to Play/Pause, Next or Previous. No permission is
needed for this.

**Your own intent**, so anything else can listen: Tasker's Send Intent action,
with an action string of your choosing. That keeps the band signal and what it
means in separate places, the way the macOS config file does.

With those two, Gadgetbridge and Tasker cover the whole job and nothing has to
be written.

### Testing it

Make the task a single Flash action showing `%EXTRA_PAYLOAD`, then press the
band button. A hex string ending `A000BB01` means every piece works, and the
rest is only choosing what to run. Nothing appearing means either the version
is too old, the per device Intent API is off, or the package is unset and the
receiver is a manifest one. See **One real trap**.

## The relay in this repository

Key Mapper cannot listen for a broadcast. It can only be triggered by one aimed
at it, so something must sit between Gadgetbridge and Key Mapper. `android/`
holds that piece, Mi Band Bridge: band events from Gadgetbridge in, Key
Mapper key maps out.

### Build and install

The APK is on the release page, as Downloads in `README.md` says. To build it
from this repository:

```sh
cd android
ANDROID_HOME=~/Library/Android/sdk ./gradlew testDebugUnitTest assembleDebug assembleRelease
mkdir -p dist
cp app/build/outputs/apk/release/app-release.apk dist/Mi-Band-Bridge-2.4.0-android.apk
```

The APK's name matches the desktop downloads on the release page. Before 2.4.0
it was `bandbridge-relay-<version>.apk`. The release build is signed with this
Mac's debug key, the key that signed every version since 1.0.0. Android installs
an update only over an app signed with the same key, so 2.4.0 installs over any
of them and keeps the rules. An APK signed with another key must uninstall the
app first, and the rules go with it: export them before, and import them after
(see **The rules backup**). `dist/` is not committed.

The Viwoods reader's adb is locked by the vendor, so the APK reaches it over the
local network. On the Mac, from the repository's root:

```sh
python3 -m http.server 8000 --directory android/dist
```

Then open `http://<the Mac's address>:8000/Mi-Band-Bridge-2.4.0-android.apk` in
the reader's browser, and install the file it downloads. The first time, Android
asks to let the browser install apps. The Mac's address is in System Settings,
Wi-Fi, Details.

Then open Mi Band Bridge once. On the reader a manifest receiver never fires,
so opening the app is the sure way to start its listener again after an update.

The debug build is for the emulator. It takes `--es fixture <name>` on launch
and seeds the design's example data: `working`, `empty`, `gate`,
`gate-keymapper`, `long`, `test-lost`, `kit`, `menu` and `imported`.
`tools/shoot.sh <fixture> <out.png> [phone|tablet]` builds it, installs it on
`emulator-5554`, and saves a screenshot. The release build holds no fixtures.

### What it does

The app has four pages:

| Page | What it is for |
|---|---|
| Main | the last five band presses, and the rules, with their export and import |
| Rule editor | a rule's signal, name and key map id, and Learn from the band |
| Setup | the two apps it needs and the band app, how to connect Gadgetbridge, how to find a key map id, and About: the app's name and version, and its licenses and notices |
| Check | whether a band press reaches this app, and if not, which side is at fault |

From 900 dp wide, Main shows two columns: the events on the left, and on the
right the rules, or the open editor, Setup or check. Turning the screen keeps
the page as it is. A change of font size or of dark mode starts the window
again, on Main or Setup, and a rule not yet saved is lost.

**The gate.** The relay needs Gadgetbridge, any of its builds
(`nodomain.freeyourgadget.gadgetbridge` and its `.nightly`, `.banglejs` and
`.nightly_nopebble`), and Key Mapper (`io.github.sds100.keymapper`). While one
is missing, the app opens on Setup, marked "Not set up", with no way back. Each
missing app has two download links: F-Droid and Codeberg for Gadgetbridge,
F-Droid and GitHub for Key Mapper. There is no Google Play link. The app asks
again each time it comes to the front, and opens on Main in place of Setup once
both are installed.

**Rules.** A rule is a signal, a name, and a key map id, all three required. The
signal follows the rule `src/signal.rs` applies on the Mac: hex that starts
with `bb`, 2 to 8 bytes, stored in lower case. So a signal that works in one
place works in the other. A press sends every rule on its signal, in list
order, once per distinct key map id. Tap a rule to change it. × removes it,
after asking, and cannot be undone.

The rules are JSON in the `relay` shared preferences, under `rules_v2`. The
first time the app starts after an update from 1.0.0, it turns 1.0.0's
`signal=uid` lines into rules named after their signal, such as "bb01". A line
with no key map id is dropped: it could never run anything. The old keys
`rules`, `last_payload`, `last_at` and `log` are removed.

**The rules backup.** The ⋮ button beside Add a rule opens a small menu: Export
rules and Import rules. Each opens Android's own file dialog, which grants the
app the one file you pick, so the app needs no storage permission. Export rules
suggests the name `mi-band-bridge-rules-<yyyy-MM-dd>.json`, with today's date,
and writes every rule to the file you choose, in list order. It is gray while
there are no rules. Import rules offers files of any type and any name, because
a browser or a messenger may save a rules file under another one. What the file
holds decides, so a file named before 2.4.0, `bandbridge-rules-<date>.json`,
imports too.

An import only adds. Each rule of the file that is not in the list yet goes at
the end, in the file's order. A rule already in the list never changes and
never goes. There is no review step and no undo. The result shows as a line
under the Rules header, such as "Imported 2 rules from
mi-band-bridge-rules-2026-09-27.json.", until the next action on the rules:
the ⋮ menu, Add a rule, a tap on a rule, or ×. A band press, Clear, turning the
screen and a visit to Setup keep it. The line is not stored, so it is gone when
the app starts again.

The file is JSON, with its keys in this order. `exportedAt` is UTC, to the
second. The file calls the key map id `keyMapId`.

```json
{
  "format": "bandbridge-relay-rules",
  "version": 1,
  "exportedAt": "2026-09-27T09:42:10Z",
  "rules": [
    {
      "signal": "bb01",
      "name": "Page down",
      "keyMapId": "3f2a9c1e-7b4d-4e0a-9c61-2d8e5f0a1b3c"
    }
  ]
}
```

What an import does with each kind of file:

| In the file | What the import does |
|---|---|
| anything but one JSON object with `format` `bandbridge-relay-rules`, `version` 1 and a `rules` list | imports nothing: "This file is not a Mi Band Bridge rules file. Nothing was imported." |
| more than 1 MB, or more than 1,000 rules | the same |
| an entry that is not an object, lacks a signal, a name or a key map id, or has one longer than 100 characters | skips that entry, and imports the others |
| the same signal, name and key map id as a rule in the list, or as an earlier entry, not counting spaces at either end or the signal's case | skips it: it is an exact copy |
| a signal that does not follow the signal rule | keeps it as typed, trimmed and in lower case, so you can see it and fix it |
| more rules than the list has room for | stops adding when the list holds 1,000 rules |

"Imported 0 rules from …" gives no reason. It means every entry was skipped:
each was a copy, a broken entry, one with a field over 100 characters, or one
past a full list. So a list that holds an exact copy of a rule exports both
copies, and an import adds only one of them. The key map is sent once either
way.

Four more lines can show. "Could not write the file. Nothing was exported." and
"Could not read the file. Nothing was imported." when the storage fails. "This
device has no app to save files." and "This device has no app to open files."
when nothing on the phone answers Android's file dialog, which an e-ink reader
may lack.

A rule runs only while Key Mapper holds a key map with its id. On a new phone,
Key Mapper's own import keeps the ids only when it replaces the key maps. Its
Append import gives them new ids, and the rules imported here then run nothing.
The design's note read this in Key Mapper 4.5.0's source.

**A custom band app.** The relay needs a custom band app on the band, such as
this repository's BandBridge band app, which sends `bb01`. Mi Band Bridge on the
Mac installs one (the README, "Install or remove an app"), and its band app
editor makes one of your own buttons (`docs/EDITOR.md`). Only the presses of a
band app show and count: a band app sends a signal, and only a signal can run a
rule. The band's own buttons are dropped, the volume keys and the other keys of
its music screen among them. So a band with no custom app sends the relay
nothing it keeps, and the check cannot pass. Setup says so in a notice under the
required apps.

**Events.** Main shows the last five band app presses, newest first: the chip,
which is the press's signal, what the press did, and the time. "Triggered Page
down, Next slide" names the rules a signal ran. "No rule for this signal"
offers "Add a rule for bb03". Clear empties the list. Five is enough to find a
fault, and keeps the rules on the phone's first screen.

Everything else the band sends is its own traffic with Gadgetbridge, and the
relay drops it as it arrives: encrypted chunks, continuation chunks, the
band's acknowledgements, chunks on any other endpoint, the keys of the music
screen and the screen opening or closing, and the band's replies on the app
endpoint. It makes no row, the check and Learn never see it, and Setup's last
press time does not move. `docs/PROTOCOL.md` has the chunk layout and the
endpoints. A row that an earlier version stored for such a payload still shows
after the update, "Not a band app signal" with its raw bytes under it and the
chunk's first two bytes as its chip, until presses push it out: `030B` for
traffic that 2.1.0 or older stored, and `0303` for a music key that 2.2.0
stored, where 2.2.0 named the key, such as `vol+`. A chip is read from the
stored bytes when the row is drawn.

**Learn from the band.** In the rule editor, Learn listens 10 seconds for one
band app press and fills in its signal. That press runs no key map and adds no
event. The band's own buttons do not end it, and make no row either. Cancel,
Back, another page, and the app going to the background all end it.

**The check.** Setup's last step. Start check watches 10 seconds for a press
in a custom band app. If one arrives: "It works.", with its signal, such as
`bb01`, and its time. The band's own traffic does not end the watch, and
neither do its own buttons: a volume key pressed during the watch leaves it
watching, and it ends in "No press arrived in 10 seconds." Then "Send a test
event" sends this app the broadcast Gadgetbridge would send, marked as a test.
If the test arrives within 3 seconds, the fault is in Gadgetbridge, and the
page lists what to check: first that the press was in a custom band app, not
one of the band's own buttons, then the four settings. If it does not, the
fault is in this app. Nothing runs a key map during a check, and a test event
never runs one, at any time.

**Setup's steps.** Under the two required apps, a notice says that the band
also needs a custom band app, which the desktop app installs, and shows the
release page's address,
`https://github.com/ctrl-alt-delete-8/mi-band-bridge/releases/latest`. A tap on
the notice opens it in the browser. A second notice is for a person without the
desktop app: "Without the desktop app, download the sample band app,
BandArrows, from the same page, and install it with Gadgetbridge." Both show on
the gate's Setup too.

"Connect Gadgetbridge" takes seven steps: connect the band, open BLE Intent
API, switch on its broadcast, copy the two values, quit Gadgetbridge from its
side menu and open it again, keep the "Band relay" notification, and run the
check with a press in the band app. Quit is in the menu that ☰ opens on
Gadgetbridge's main screen. It stops Gadgetbridge's service and ends its
process, so Gadgetbridge reads the two values again when it starts. A swipe in
the recent apps screen closes only its window, and the service runs on with the
values it read before. The two values have copy buttons, because one extra
space stops every event (see **The trap that cost an evening**):

| Gadgetbridge field | Value |
|---|---|
| BLE Characteristics | `00000017-0000-3512-2118-0009af100700` |
| BLE API package | `dev.bandbridge.relay` |

"Find a key map ID" then takes four steps. Under Gadgetbridge's row, Setup says
when the last band press arrived. Clear on Main does not reset that time, and
the band's own traffic does not move it.

**The "Band relay" notification.** The relay runs a foreground service with one
low-importance notification, in the channel "Band relay": "Listening for band
presses". The service registers a receiver in code, which hears Gadgetbridge
whether or not the BLE API package names this app. The manifest also declares
the receiver, so Android starts the app for a broadcast aimed at it by name.
The service starts with the window. It also starts after the phone restarts and
after the app is updated, through a manifest receiver for each, so not on the
reader, where no manifest receiver fires: there, open the app after either.
After a force stop of the relay, open it to start the service again: a press
may wake a stopped app on some phones, but Android does not let the app start
its service then. On Android 13 and later the app asks once for permission to
show notifications, the first time Main appears. Without it the service still
runs, but its notification does not show. Allowed later in Settings, the
notification shows the next time the app comes to the front.

**One press, two deliveries.** A broadcast aimed at this app reaches both
receivers. The same payload twice within 800 ms counts once, so each key map is
sent once and the press makes one event.

Key Mapper is triggered like this:

```text
component  io.github.sds100.keymapper/io.github.sds100.keymapper.api.TriggerKeyMapsBroadcastReceiver
action     io.github.sds100.keymapper.ACTION_TRIGGER_KEYMAP_BY_UID
extra      io.github.sds100.keymapper.EXTRA_KEYMAP_UID
```

Key Mapper 4.5 has no "Trigger from other apps" switch: any key map can be
triggered this way. Its id is in the key map's Options tab, under Information,
as Key map ID. In Key Mapper 4.4 or older, first turn on "Allow other apps to
control this key map with intents or shortcuts" in the same tab. The component
must be named: Android drops the intent otherwise, and reports nothing.

Android 11 and later also hide other apps unless the manifest names them, so
the relay carries a `<queries>` entry for Key Mapper, and one for each
Gadgetbridge build, for the gate. Without the Key Mapper entry the broadcast is
dropped in the same silence.

## With your own app

```kotlin
class BandReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val payload = intent.getStringExtra("EXTRA_PAYLOAD") ?: return
        when {
            payload.endsWith("A000BB01") -> onButton()
            payload.endsWith("A000BB02") -> onUp()
            payload.endsWith("A000BB03") -> onDown()
        }
    }
}
```

```xml
<receiver android:name=".BandReceiver" android:exported="true">
    <intent-filter>
        <action android:name="nodomain.freeyourgadget.gadgetbridge.ble_api.events.CHARACTERISTIC_CHANGED" />
    </intent-filter>
</receiver>
```

## One button, a different action in each app

Key Mapper cannot do this inside one key map. Read in its v4.4.0 source:
constraints belong to the key map (`KeyMap.constraintState`), an `Action` has
no constraint field, and no action type branches on a condition. A key map
runs all its actions or none.

But a key map triggered by another app still checks its constraints.
`SimpleMappingController.onDetected` returns before any action runs when they
fail:

```kotlin
if (keyMap.constraintState.constraints.isNotEmpty()) {
    val constraintSnapshot = detectConstraintsUseCase.getSnapshot()
    if (!constraintSnapshot.isSatisfied(keyMap.constraintState)) return
}
```

So make one key map per app, and let the relay send them all:

1. In Key Mapper, make a key map for each app. Give each one the constraint
   **App in foreground** for that app, and copy its key map id from the Options
   tab.
2. In the relay, add one rule per key map, all with the same signal.
3. Press the band button. The relay sends every key map for that signal. Key
   Mapper runs the one whose app is in front and skips the rest.

| Signal | Key map | Constraint in Key Mapper |
|---|---|---|
| `bb03` | page down | app in foreground: the reader |
| `bb03` | scroll down | app in foreground: the browser |

For a default in every other app, add a key map with **App not in foreground**
for each of the other apps, in AND mode.

A key map listed twice for one signal is sent once.

## On a Viwoods AiPaper, the background listener is required

Measured on the device, 2026-09-06: a press reaches the relay only through the
receiver its service registers in code. The manifest receiver does not fire,
whether the broadcast is implicit or explicit.

Why is not established. There is no way to read logcat on this reader, because
the vendor restricts adbd. It is consistent with the aggressive background
management these e-ink ROMs use, where an app the system has stopped is not
woken for a broadcast. On an ordinary phone the manifest receiver should work
on its own.

So the listener is not a setting: the relay always runs its service, and Setup
asks you to keep its "Band relay" notification. It costs that one
low-importance notification. The service polls nothing, holds no wake lock, and
wakes only when a broadcast arrives.

With the service running, the BLE API package field may even be left empty,
which sidesteps the trimming trap below on Gadgetbridge 0.93.0 and older. Setup
still shows it to copy, for the phones where the manifest receiver works.

## The trap that cost an evening

**Gadgetbridge 0.93.0 does not trim the BLE API package field.**

```java
this.intentApiPackage = devicePrefs.getString(PREFS_KEY_DEVICE_BLE_API_PACKAGE, "");
```

A trailing space makes it call `setPackage("dev.bandbridge.relay ")`. Android
then delivers the broadcast only to a package of that exact name, which does
not exist, so nothing receives it: not a manifest receiver, not a receiver
registered in code, not a service. It looks identical to Gadgetbridge never
broadcasting at all, and nothing logs a word.

Gadgetbridge 0.94.0 fixes this. Its package field takes several packages,
comma separated, and it trims each one:

```java
this.intentApiPackages = devicePrefs.getList(PREFS_KEY_DEVICE_BLE_API_PACKAGE, Collections.emptyList())
        .stream().map(String::trim).filter(s -> !s.isEmpty()).collect(Collectors.toList());
```

On 0.93.0 and older, copy the package from the relay's Setup, and check the end
of the line.

Three things that are easy to blame instead, and were all innocent here: the
Gadgetbridge version, the Android permissions, and the ROM's battery settings.

## How to read the Gadgetbridge log

Turn on **Write log files** in Gadgetbridge's settings. The log answers the
questions no screen does:

| Line | Meaning |
|---|---|
| `Initializing queue ... with threadIdx=N` | a new device support object. The intent API is built here and nowhere else |
| `BleIntentApi - Subscribed to filtered <uuid>` | the API is armed for this connection |
| `characteristic changed: 00000017-... - ...A000BB01` | a band press arrived |

If the `Initializing queue` line is missing after a settings change, the
settings have not been read. A disconnect and reconnect inside the app reuses
the old object: quit Gadgetbridge from its side menu instead, and open it again.

If `Subscribed to filtered` is missing, the notify switch is off, or the
characteristic filter is empty. An empty filter is worse than a wrong one on
0.93: it tries to subscribe to every characteristic, write-only ones included,
and a failing GATT action takes the rest of device init with it. 0.94.0 fixes
that too, as "BLE Intent API: Prevent abortion of device init".

## One real trap

With `prefs_device_ble_api_package` empty, Gadgetbridge sends an implicit
broadcast. Android 8 and later deliver an implicit broadcast only to a receiver
registered at run time, never to one declared in a manifest. A manifest
receiver will look dead, and nothing will say why.

Naming your package makes the broadcast explicit, and an explicit broadcast
does reach a manifest receiver. So either name the package, or register the
receiver in code with `registerReceiver`.

## Sending to the band

The same API works the other way, which this project has no equivalent for:

```text
action  nodomain.freeyourgadget.gadgetbridge.ble_api.commands.CHARACTERISTIC_WRITE
extras  EXTRA_DEVICE_ADDRESS
        EXTRA_CHARACTERISTIC_UUID   00000016-0000-3512-2118-0009af100700
        EXTRA_PAYLOAD               hex bytes
```

Write to `0x16`, the chunked write characteristic. Building a valid chunk is
your job, and `src/protocol/chunk.rs` in this repository is the reference.
Anything encrypted needs the session key, which Gadgetbridge holds and does not
hand out, so keep to unencrypted endpoints.

## What Android does not give you

There is no equivalent of posting a key event. `CGEvent` has no counterpart
that an ordinary app may use. To act on another app you need an
`AccessibilityService`, and to send media keys you use `AudioManager` or a
`MediaSession`. So the macOS actions do not port. Bind to intents and actions
instead, which is the Android idiom anyway.

## Other intents worth knowing

Gadgetbridge accepts these, each switched on separately in its settings:

| Action | Extras |
|---|---|
| `...gadgetbridge.BLUETOOTH_CONNECT` | `EXTRA_DEVICE_ADDRESS` |
| `...gadgetbridge.BLUETOOTH_DISCONNECT` | `EXTRA_DEVICE_ADDRESS` |
| `...gadgetbridge.action.SET_DEVICE_SETTING` | `device`, `key`, `value` |
| `...gadgetbridge.command.ACTIVITY_SYNC` | `dataTypesHex` |

and broadcasts `...gadgetbridge.BLUETOOTH_CONNECTED` with
`EXTRA_DEVICE_ADDRESS`.

`SET_DEVICE_SETTING` reaches the config service this project probes on endpoint
`0x000a`. The 15 second screen-on cap applies there too: it is enforced by the
band, not by the host.

## Measured, and not

The Gadgetbridge parts of this file are read from its source at the versions
linked in the commit that added it. Treat the intent names and preference keys
as exact, and the behavior as expected rather than confirmed, except where a
section says it was measured.

The relay has run on the reader, a Viwoods AiPaper on Android 13. Version 2.0.0
was also checked on an Android 16 emulator, with Gadgetbridge 0.94.0 from
F-Droid and Key Mapper 4.5.0 from GitHub: it installed over 1.0.0 and kept the
rules, a press sent by adb reached Key Mapper's receiver, and removing Key
Mapper brought back the gate.

Version 2.1.0 was checked on the same emulator. It installed over 2.0.0 and
over 1.0.0, and kept the rules. The rules went to a file and back through
Android's own Save and Open dialogs, in the same order, and files that were
broken, too large or not this format imported nothing and changed no rule.
Quit was read from the side menu in Gadgetbridge 0.94.0's own resources, and
not tried: the emulator has no band.

Version 2.2.0 was checked on the same emulator, with presses and band traffic
sent by adb as Gadgetbridge broadcasts them. It installed over 2.1.0 and kept
the rules, in order, and the traffic rows that 2.1.0 had stored still showed
until presses pushed them out. Encrypted chunks, continuation chunks,
acknowledgements, other endpoints, the music screen opening and closing, and a
reply on the app endpoint made no row, did not end a check's watch, and left
Setup's last press time as it was. Each of the six music buttons made its row
and ran no key map, the check took a volume key, and Learn did not.

Version 2.3.0 was checked on the same emulator, the same way. It installed
over 2.2.0 and kept the rules, in order, and the rows that 2.2.0 had stored for
the volume keys still showed, with `0303` as their chip, until presses pushed
them out. The six keys of the music screen made no row, ran no key map, and
left Setup's last press time as it was. Volume down during a check's watch
left it watching, until it ended in "No press arrived in 10 seconds." Volume
up during Learn left the signal empty. A band app press still made its row,
ran its key maps, ended a check with "It works.", and filled in Learn's
signal. Setup showed the band app notice, both as the gate's root and from
Main.

A real band press through Gadgetbridge on the reader, the band's own traffic
and its music keys, Quit, and the reader's own file dialogs are left to a
person, as checks by hand.
