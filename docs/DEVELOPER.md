# Mi Band Bridge for developers

This file is not a user entry point. A person uses the Mi Band Bridge window for
everything: pairing, the connection, the settings, the event log, the actions,
the apps on the band, and the band's screens. The README describes the window.

This file is for work on the code: the repository, the build, the command line
tool that probes the protocol, the socket, the band apps, and the logs.
`docs/PROTOCOL.md` records what has been measured on the band. Comments in the
code name design decisions by number, such as D9. The notes that hold them are
not published.

## The repository

| Path | What it holds |
|---|---|
| `src/` | the library that talks to the band, and the command line tool (`src/main.rs`, `src/cli.rs`) |
| `gui/` | the window, `bandbridge-gui`, a Tauri app. `gui/ui/` is its page |
| `editor/` | the band app editor, `bandbridge-editor` (`docs/EDITOR.md`) |
| `android/` | the Android relay (`docs/ANDROID.md`) |
| `band-app/`, `band-app-arrows/` | the two band apps this project ships |
| `apps/` | band apps made with the editor |
| `examples/` | protocol probes |
| `scripts/` | the build and install scripts |
| `packaging/Info.plist` | the app bundle's Info.plist |

## Build and install

```sh
./scripts/build.sh --release    # build and sign target/Mi Band Bridge.app
./scripts/install-app.sh        # copy it to /Applications
```

`build.sh` runs `cargo build`, which makes the library and the command line
tool, and `cargo build -p bandbridge-gui`, which makes the window. It puts the
window alone in `target/Mi Band Bridge.app`, as `Contents/MacOS/bandbridge-gui`,
and signs the bundle. The window is the only entry point (D9), so the bundle
holds no command line tool.

Do not use a window that `cargo build` made on its own. The script signs the
bundle, and macOS records the Bluetooth and input permissions against that
signature.

No Apple account is needed. The script picks the signing identity in this
order:

1. `BANDBRIDGE_SIGN_IDENTITY` in the environment
2. the first line of `~/.config/bandbridge/sign-identity`
3. `-`, an ad hoc signature

An ad hoc signature names a hash of the binary, so macOS asks for Bluetooth and
input permission again after every rebuild. A certificate keeps one identity
across builds, so a grant survives. A free Apple account in Xcode provides one.
Run `security find-identity -v -p codesigning` to list yours.

The Keychain item that holds the auth key trusts the app by that identity too.
A build signed another way cannot read the key without a Keychain dialog, so
keep `~/.config/bandbridge/sign-identity`.

`install-app.sh` quits the copy that runs, then replaces it. It copies to
`/Applications`, or to `~/Applications` when `/Applications` is not writable.
It also removes an old `BandBridge.app`, the app's name before version 1.0.0,
so two copies never hold the band. Open Mi Band Bridge again afterwards.

Mi Band Bridge must keep running with its window closed. Closing the window only
hides it, but macOS quits a menu bar app with no window on screen by itself,
soon after (automatic termination). On macOS 27 that took about two seconds,
and dropped the band. So `packaging/Info.plist` sets
`NSSupportsAutomaticTermination` to false, and the window turns automatic
termination off as it starts (`keep_running_without_a_window` in
`gui/src/main.rs`). To check it, close the window and read AppKit's own log:

```sh
/usr/bin/log show --last 5m --style compact \
  --predicate 'process == "bandbridge-gui" AND category == "AutomaticTermination"'
```

`_kLSApplicationWouldBeTerminatedByTALKey=1` means macOS may quit it.

Mi Band Bridge must not keep the focus after a desktop switch. macOS gives the
focus to the last active app of a desktop when you return to it, and Open Mi
Band Bridge makes Mi Band Bridge active, so that app can be Mi Band Bridge. A
band press would then type into Mi Band Bridge. `gui/src/desktop_switch.rs`
watches for a desktop switch (`NSWorkspaceActiveSpaceDidChangeNotification`) and
for Mi Band Bridge becoming active (`NSApplicationDidBecomeActiveNotification`).

- When the switch's notice comes within 1 second after Mi Band Bridge became
  active, the order this Mac shows, Mi Band Bridge hands the focus back at the
  notice. The switch has settled then, so the window it picks is on the new
  desktop.
- When Mi Band Bridge became active within 1 second after a notice, it waits 0.5
  seconds for the switch to settle, then hands the focus back. If a second
  notice comes first, from a second quick switch, it hands the focus back at
  that notice.
- Each notice and each activation counts once.
- Mi Band Bridge keeps the focus when a click in its window made it active, and
  when Open Mi Band Bridge, or opening the app again, ran in the 2 seconds
  before.

It hands the focus to the app that owns the frontmost ordinary window: a window
on layer 0 that can be seen, of an app with a Dock icon, and not Mi Band
Bridge's. With a desktop for each display, the window must also be on the
display of Mi Band Bridge's window. With no such window, Mi Band Bridge keeps
the focus. A click on Mi Band Bridge's window in Mission Control may also hand
the focus back once. The window logs one line each time a switch made it active:
the app it gave the focus to, or why it kept it (see "Logs").

### The Mac release

```sh
./scripts/package-mac.sh
```

It makes `dist/mac/Mi-Band-Bridge-<version>-mac-arm64.dmg`, which holds Mi Band
Bridge, the Mi Band App Editor and a link to `/Applications`. Both apps are for
Apple Silicon only. The script builds both in release mode: the bridge with
`build.sh`, and the editor with Tauri's bundler, through the same Tauri command
line tool that `build-windows.ps1` uses, built into `target/tauri-cli`. It signs
each app with the Developer ID and the hardened runtime, sends it to Apple's
notary service, and staples the ticket to it. Then it does the same for the
disk image. Last, it checks a copy of the image, and of each app in it, that
carries the quarantine flag a download gets: Gatekeeper (`spctl`) must accept
each one, and each must carry its ticket. It installs nothing.

It writes each notary submission's id and status to `dist/mac/notarization.txt`,
and the image's SHA-256 to a `.sha256` file beside the image. When Apple
rejects a file, the script prints Apple's log for it and stops.

It needs a "Developer ID Application" certificate in the Keychain, and a
notarytool keychain profile named `mi_band_mac_app`, which holds the Apple ID's
app-specific password. The script names only the profile, so it never reads or
prints the password. `MAC_RELEASE_IDENTITY` and `MAC_NOTARY_PROFILE` override
the two names: set `MAC_RELEASE_IDENTITY` to the certificate's full name when
the Keychain holds more than one Developer ID Application certificate. Make the
profile once:

```sh
xcrun notarytool store-credentials mi_band_mac_app --apple-id <you> --team-id CYS5988KRM
```

The hardened runtime blocks what the app's entitlements do not allow. Bluetooth,
the input permission and the Accessibility permission are permissions the
person grants, not entitlements, so the bridge needs no entitlement for them.
It needs one, `com.apple.security.automation.apple-events`, in
`packaging/bridge.entitlements`. An action's command or script may send Apple
events, such as `osascript -e 'tell application "Music" to pause'`, and under
the hardened runtime macOS holds the app responsible for its child processes.
Without the entitlement, that script fails with error -1743.
`NSAppleEventsUsageDescription` in `packaging/Info.plist` is the reason macOS
shows when it asks. The editor needs no entitlement.

To macOS, a Developer ID build has a new signature. The first one asks again for
Bluetooth, the input permission and Accessibility, and the Keychain asks once
whether Mi Band Bridge may read the band key. macOS keeps a grant by the app's
designated requirement. For a Developer ID build, that requirement names the
team (CYS5988KRM) and the bundle identifier, not one certificate, so a later
Developer ID build satisfies it too. Check one with `codesign -d -r- <app>`.

## The auth key

The key is in the login Keychain: service `BandBridge`, account the band's MAC
in upper case with colons, such as `00:00:5E:00:53:01`. The window puts it
there when it pairs a band, and deletes it when it forgets one. No file holds
it, and nothing prints it: not a log line, not a reply on the socket, not an
error, not the window's page.

A process looks for the key in `BAND_AUTH_KEY` first, then in the Keychain
under `[device] mac`. The window looks as it starts. When it has no key then,
a Connect reads the file and looks again (`Supervisor::look_again`), so a
Keychain that answers later, or a file fixed since, needs no restart. A
Keychain with no key gives the advice to forget the band and pair it again.
Any other failure gives its own reason, and never that advice: the key may
still be there. The window's Copy key reads the Keychain alone.

The item trusts the window by its signature. Any other program that reads it,
the command line tool included, gets a Keychain dialog. This prints the item's
attributes, and never the key:

```sh
security find-generic-password -s BandBridge
```

Its `-w` option prints the key, so keep that option out of anything that keeps
a log.

## The config file

`~/Library/Application Support/BandBridge/config.toml` holds the device, the
settings and the actions. The README shows the format. `src/config.rs` loads
and saves it through one set of types, so the file the window writes is the
file it reads.

The window writes it. Every write in the process takes one lock, reads the
file, changes it, writes the result beside it, and renames that over the file,
so no reader ever sees half a file. A file that does not load is never
written: a change to one part must not replace a part it could not read.

The band task follows the file. It looks at the file on each pass of its loop,
and again at the moment each part is used: the actions at a press, auto
reconnect at a drop, and new app on top at the end of an install and again as
the link comes back after it. So a hand edit counts from the next thing that happens, with no
reconnect and no restart. `[device]` is the exception: the band task takes it
up as it starts, at a pair, and at a Connect while it holds no key, such as
after a start with a file that did not load.

## The command line tool

`cargo build` builds it as `target/debug/bandbridge`, and
`cargo build --release` as `target/release/bandbridge`. It is a developer's
tool for protocol probes. It is not in the app bundle and not on the PATH, and
it gets no new features (D9). Nothing in the window or the band app editor
runs it.

It reaches the band in one of two ways.

**Through Mi Band Bridge's socket.** While Mi Band Bridge runs, `device`,
`follow`, `apps`, `screens` and `install` send their request to it, and it
answers on the link it holds. They open no Bluetooth link, and they read no key.
This is the way to use the tool, and the only way that has been measured since
it left the bundle.

**On its own Bluetooth.** `scan`, `inspect` and `listen` always use Bluetooth
themselves, and `apps`, `screens` and `install` do when nothing answers on the
socket. The band takes one host at a time, so quit Mi Band Bridge from its menu
bar item first, and open it again afterwards. This path is not measured since
the tool left the bundle, and it is probably broken:

- cargo leaves the tool the linker's ad hoc signature, which does not bind the
  Info.plist that `build.rs` embeds. macOS then ignores the Bluetooth usage
  description, and kills the process the moment it scans (`docs/PROTOCOL.md`,
  "macOS code signing").
- The Keychain item that holds the key trusts the window alone, so the tool gets
  a Keychain dialog when it reads the key, and again after each rebuild.

Neither matters to a person, who uses the window, so neither is fixed.

### The input permission, for listen

A `listen` session posts keystrokes itself, so macOS must allow it. macOS
judges that against the responsible process: the program that launched the
tool, such as the terminal, and not the tool. Open System Settings, Privacy &
Security, Device Control and Data Access, and switch that program on. Before
macOS 27 the pane is called Accessibility. `listen` refuses to start when an
action posts a keystroke and the permission is missing, and names the pane.

To check without starting a session, build a two line probe, and run it from
the same shell:

```swift
import ApplicationServices
print(AXIsProcessTrusted())
```

If it prints false, fix the permission before changing anything else.

### scan

```sh
target/release/bandbridge scan [--seconds 6]
```

Lists nearby peripherals and their advertised names. A band that a Mac holds
does not advertise. Pairing finds the band from its key, so nothing needs the
ids this prints.

### inspect

```sh
target/release/bandbridge inspect <PERIPHERAL_ID> [--seconds 20]
```

Prints one peripheral's services and characteristics. The id is one that
`scan` printed, or `[device] uuid` from `config.toml`. Use it to confirm that
the band exposes the Zepp service before you look for faults elsewhere.

### listen

```sh
target/release/bandbridge listen [--raw] [--no-auto-reconnect] [--subscribe-all] [--mtu N] [--peripheral-id ID] [--seconds 20]
```

Holds the band from a terminal, as the window does. It connects,
authenticates, prints the band's events, and fires the actions in
`config.toml`. It serves the socket too, so the other commands reach it as
they reach the window. It finds the band by `[device] uuid`, or by name when
there is none.

It says how many actions are active, then prints what `follow` prints, from
the same function:

```text
3 action(s) active
[state] ready · connected (started) since 2026-09-25T07:11:47-04:00
[app] bb02
[info]   fired Page up (page_up)
```

Type a letter and press Return to control the session:

| Key | Action |
|---|---|
| `c` | connect |
| `d` | disconnect and stay disconnected |
| `s` | print the current state |
| `q` | release the band and exit |

A manual disconnect stops auto reconnect until you type `c`. This matters
because a session that reconnects on its own starves your phone.
`--no-auto-reconnect` holds for the whole run, whatever `config.toml` says.

Add `--raw` to print every GATT notification and decoded chunk. Use
`--subscribe-all` only to hunt for an endpoint that is not on the usual
characteristic.

### apps

```sh
target/release/bandbridge apps                   # the apps installed from here
target/release/bandbridge apps --all             # every package the band holds
target/release/bandbridge apps --remove <ID>     # take one off the band
```

The list shows the apps this Mac can name, which are the apps installed from
here, and says how many other packages the band holds without listing them.
The watch face and whatever came with the band have no name here, and removing
one is either refused or a bad idea. The band keeps no copy of a removed app.

### install

```sh
target/release/bandbridge install <FILE> [--peripheral-id ID] [--seconds 20]
```

`FILE` is the `.bin` from a build, such as `band-app/dist/band-app.bin`. This
never sends firmware.

Through Mi Band Bridge, the install runs on the link the window holds, and the
window's event log shows it. The install ends that session, because it
unsubscribes the chunked transport, so the window connects again. New app on
top then decides whether the new app goes first in the app menu, as it does
for an install from the window. Nothing else in the menu moves.

With nothing on the socket, the tool installs on a link of its own. Then it
always rebuilds the app menu on a second short link: every installed app, the
new one first, then settings. It does not read New app on top.

### screens

Reads and arranges the band's three screen lists. The window arranges the app
menu and the shortcuts. The control center is left to this command (D5).

```sh
target/release/bandbridge screens                                    # read all three
target/release/bandbridge screens --list menu --hide music           # drop an entry
target/release/bandbridge screens --list shortcuts --set hr,weather  # exact order
target/release/bandbridge screens --list menu --tidy                 # apps, then settings
```

| Option | Meaning |
|---|---|
| `--list` | `menu`, `shortcuts`, or `control`. Default `shortcuts` |
| `--set` | replace the shown entries with exactly these, in this order. With nothing after it, show nothing |
| `--hide` | remove these from the shown entries, keep the rest in order |
| `--tidy` | rebuild the menu as every installed app, then settings |

Give names such as `music`, `weather`, or `workout`. Ids also work. A run with
no `--set`, `--hide` or `--tidy` reads all three lists, whatever `--list`
says, and prints each one in turn: the menu, the shortcuts, then the control
center. A few lines of the menu's part:

```text
menu (28 entries)
  hidden   6  hr (00000002)
  hidden   7  sleep (00000023)
  shown    0  BandArrows (app)
  shown    1  BandBridge (app)
  shown    3  settings (00000013)
```

An entry marked `(app)` is one of your installed apps. The band sends no name
for those. The name comes from `app-names.toml`, the one record of app names:
each install through Mi Band Bridge reads the id and the name from the manifest
beside the package, and adds them there. An app with no entry shows its id.

Three limits, all measured on the band:

- The band takes at most ten shortcuts.
- Settings cannot be removed from the menu.
- An installed app cannot become a swipe shortcut. Shortcut cards need Zepp OS
  API level 2.0, and this band is 1.0. The band discards such a write whole.

The window refuses the first two before it sends anything (`gui/ui/arrange.js`).

Every write reads the current list first and sends it back complete. Each entry
carries a flags byte and strings that this program does not interpret, and
rebuilding a list without them loses whatever the band tracks there.

### device

```sh
target/release/bandbridge device
```

Prints what the holder knows about the band, as JSON:

```text
{"battery":{"charging":false,"percent":78,"secondsAgo":58},"connectedSince":"2026-09-25T07:11:47-04:00",
 "firmware":"2.2.0.1","hardware":"0.91.130.5","mac":"00:00:5E:00:53:01","name":"Xiaomi Smart Band 7",
 "serial":"12345/ABCD..."}
```

The holder reads the address and the versions once per link, from the standard
Bluetooth services. It asks for the battery when the link comes up and at each
request, and the band reports a change of level on its own, so `secondsAgo`
says how old the level is. `name` comes from `[device]`. `connectedSince` is
when the link came up, and is `null` while the band is not connected.

macOS never gives an app a Bluetooth address. CoreBluetooth hands out a UUID
of its own, which is `[device] uuid`. The address here comes from the band,
out of the System ID characteristic. A connect lets a band go before the
handshake when that address is not `[device] mac`, but a band whose System ID
holds no address, or does not answer, skips this check, and only the key
stops it (`open_link` in `src/supervisor.rs`).

### follow

```sh
target/release/bandbridge follow
```

Prints band events from whichever process holds the band, as `listen` prints
them. Use it to watch presses while the window has the band:

```text
[state] ready · connected (you) since 2026-09-25T00:26:17-04:00
[app] bb02
[info]   fired Page up (page_up)
[app] bb02
[info] the window learned bb02, so nothing fired
```

It shows the band's bytes and the action a press fired, not how the action
went. The window's event log shows that.

## The socket

Whichever process holds the band, the window or a `listen` session, listens on
`~/Library/Application Support/BandBridge/bandbridge.sock`. The socket is the
private link between Mi Band Bridge's own programs: the band app editor installs
through it, and the command line tool sends its requests there. Both sides call
the same library functions, so a request and a button do the same work by the
same path.

It takes one JSON request per line, and answers with JSON lines until `done`
or `failed`:

```text
-> {"request":"status"}
<- {"reply":"line","text":"state ready"}
<- {"reply":"data","json":"{\"state\":\"connected\",\"reason\":\"started\",\"since\":\"2026-09-25T07:11:47-04:00\"}"}
<- {"reply":"done"}
```

The requests are `status`, `connect`, `disconnect`, `device`, `follow`,
`apps`, `app_list`, `screens`, `screen_list` and `install`.

The socket refuses `pair`, `forget`, `saved_setup` and `copy_key` by name.
Those are the window's alone, so no other program can pair a band, forget one,
or put its key on the clipboard. The window's other commands, such as its
settings, its actions, Learn and Record keys, are not requests at all, and the
socket refuses them as unknown.

A path sent over the socket is made absolute first. The holder has a working
directory of its own, so a relative path would resolve against the wrong
place.

An install through the holder runs on the link the holder already has. A
second link from the same process cannot work: the band takes one host, and
that host is already this program.

An install answers with the package's size, then a line each time the
upload's percentage moves, such as `62%`, then `Installed.`. The band app
editor shows those lines as the percentage on its Install button.

If a process dies without cleaning up, the next one to start removes the stale
socket file.

## The band apps

Two apps ship with this project:

| App | Buttons | Signals | Package |
|---|---|---|---|
| BandBridge | one, filling the screen | `bb01` | `band-app/dist/band-app.bin` |
| BandArrows | two pills, up and down | `bb02`, `bb03` | `band-app-arrows/dist/band-app-arrows.bin` |

Both leave a strip of bare screen at the bottom, so a back swipe has somewhere
to start that is not a button.

Both hold the screen lit for 120 seconds while they are open, and hand it back
when they close. The band's own screen-on setting stops at 15 seconds. Change
`SCREEN_ON_SECONDS` in `page/index.js` to alter it.

Both also return to their own page when the band wakes, rather than to the
watch face. The band still turns the screen off when it decides it is not on a
wrist, and nothing on this band changes that: see `docs/PROTOCOL.md`.

To add a band app by hand, read `skills/band-app.md`. It says where the files
go, the layout rules, the free signal bytes, and how to build and install. For an app of plain buttons, the band app editor needs no code
(`docs/EDITOR.md`).

### Build and install an app

```sh
./scripts/make-band-assets.py       # redraw the icons and artwork
./scripts/build-band-apps.sh        # package both apps into their dist folders
```

Then install the `.bin` from the window: Install or remove an app, then
Install an app (.bin). With Mi Band Bridge open, the tool sends the same install
through the socket:

```sh
target/release/bandbridge install band-app-arrows/dist/band-app-arrows.bin
```

`build-band-apps.sh` needs zmake, which converts the PNG files under `assets/`
into the TGA subtype the band reads. A plain PNG there is ignored, and the app
shows no icon. zmake is GPL-3 and is not on PyPI, so it lives outside this
repository, in `~/.local/share/bandbridge-tools`, or in `BANDBRIDGE_TOOLS`
when that is set. Set it up once:

```sh
python3 -m venv ~/.local/share/bandbridge-tools
~/.local/share/bandbridge-tools/bin/pip install Pillow qrcode
git clone https://github.com/melianmiko/zmake ~/.local/share/bandbridge-tools/zmake-src
```

The `dist/` folders are not in git, so a fresh clone has no `.bin` until the
script runs.

Take asset sizes from the device profile in
`~/.local/share/bandbridge-tools/zmake-src/zmake/data/zepp_devices.json`, not
from the general Zepp OS design guide. That guide covers round watches. This
band wants a 100 by 100 icon on a 192 by 490 screen.

## Protocol probes

Each file in `examples/` asks the band one thing, on a link of its own:

```sh
cargo run --example device_probe
```

The others are `app_list`, `clock_probe`, `config_probe`, `config_set`,
`screen_timeout`, `services_probe` and `watchface_list`. Quit Mi Band Bridge
first, as for the tool's own Bluetooth, and expect the same problems: cargo
signs them the same way, and all of them but `device_probe` read the key.

## The preview harness

`gui/ui/preview/` runs the window's page in a browser. A stand-in for the app
answers every call from `gui/ui/preview/fixtures.json`, so each section can be
seen in each state, in both themes, beside its design frame. The header of
`gui/ui/preview/preview.js` explains the address it takes:

```sh
python3 -m http.server 8765 --bind 127.0.0.1 --directory gui/ui
```

Then open `http://127.0.0.1:8765/preview/index.html?fixtures=connected&open=all`.

## Logs

Add `RUST_LOG=bandbridge=debug` to a run of the tool to print everything, or
`RUST_LOG=bandbridge::install=debug` to print every control frame of an
install.

A window opened from the Finder, or with `open`, sends its output nowhere. To
read it, quit Mi Band Bridge, then run `"/Applications/Mi Band
Bridge.app/Contents/MacOS/bandbridge-gui"` from a terminal. A window started
from a terminal takes the terminal's input permission, not its own, so start it
that way only to read its output.

## When something fails

| Symptom | Cause |
|---|---|
| the tool dies with no message the moment it scans | its linker signature does not bind its Info.plist, so macOS kills it (see "The command line tool") |
| a Keychain dialog when the tool connects | the key's Keychain item trusts the window alone |
| `listen` refuses, naming the permission pane | the program that launched it lacks the input permission |
| `the band refused command 0xd5 with status 0x0b` | the band does not hold the whole payload yet |
| the app shows no icon | the asset is a PNG, not the TGA subtype zmake writes |
| the icon sits in a corner or overflows | the icon is not 100 by 100 |
| a keystroke with modifiers types the bare character | the modifiers are flagged on the key event, not pressed. A shortcut never sees one go down, and a text field gets the plain key |

`docs/ANDROID.md` covers the same band buttons on Android, where Gadgetbridge
holds the band and broadcasts an intent instead.
