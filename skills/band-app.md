
# Adding a band app to this project

Two apps exist already. Copy the closer one and change it, rather than starting
from nothing.

| App | Buttons | Signals |
|---|---|---|
| `band-app/` | one, filling the screen | `bb01` |
| `band-app-arrows/` | two pills, up and down | `bb02`, `bb03` |

The platform rules behind all of this are in `skills/zepp-os-app-build.md`.
Read it before changing anything about packaging, assets, or the install.

## Make a new app

```sh
cp -R band-app band-app-<name>
```

Then edit `band-app-<name>/app.json`:

- **`appId`** must be new. The ones in use are `1491417013` and `1491417301`.
- **`appName`** shows in the band's own launcher.
- **`version`** starts at code 1.

## Layout rules for this band

The screen is **192 by 490**.

- Leave **70px of bare screen at the bottom**, free of buttons. The back swipe
  needs somewhere to start that is not a control. Both apps do this with a
  `SAFE_BOTTOM` constant, and the editor makes the same strip. Under 30px, a
  swipe may press a button.
- Keep a 6px margin at the edges, so round ends stay off the bezel.
- A button's artwork must be the button's own size. The band does not stretch
  it.
- Button text renders centered, so put artwork in the upper half of a pill if a
  label sits below it.

## Keep the screen lit

The band's own screen-on setting stops at 15 seconds, which is too short to
read a screen and press a button. An app raises it for itself:

```js
var SCREEN_ON_SECONDS = 120;

function holdScreen() {
  try {
    if (typeof hmSetting !== 'undefined' && hmSetting.setBrightScreen) {
      hmSetting.setBrightScreen(SCREEN_ON_SECONDS);
    }
  } catch (e) {
    // A firmware without the call must not take the page down.
  }
}

function releaseScreen() {
  try {
    if (typeof hmSetting !== 'undefined' && hmSetting.setBrightScreenCancel) {
      hmSetting.setBrightScreenCancel();
    }
  } catch (e) {
  }
}
```

Call `holdScreen()` at the end of `onInit` and again on every press, so the lit
time starts over while a person is using the app. Call `releaseScreen()` in
`onDestroy`, or the band keeps the screen lit after the app closes and the
battery pays for it.

Pair it with `hmApp.setScreenKeep(true)` in `onInit`. That does not keep the
screen on, whatever the name suggests: it makes the band return to this page
when it wakes, instead of to the watch face.

The screen still goes off when the band decides it is not on a wrist. No app
API changes that, and the config service has no setting for it. Always-on
display is a watch face feature and shows the watch face, not an app.

`hmSetting.setBrightScreen(seconds)` takes 1 to 2147483 and returns 0 when it
worked. It is a Zepp OS 1.0 device app API, so this band has it. Guard the call
anyway: a missing function throws, and a throw in `onInit` shows a black screen
with no error.

## Pick a signal

The first byte is always `0xbb`. The second is the app's own:

```js
var buf = new Uint8Array([0xbb, 0x04]);   // bb04, free in this repository
hmBle.send(buf.buffer, buf.byteLength);
```

Mi Band Bridge sees the raw bytes on endpoint `0x00a0` and reads them as
lowercase hex, so `bb04` is the signal an action names.

Then say so. Every app declares what it sends in `signals.json`, beside
`app.json`:

```json
{
  "signals": ["bb04"]
}
```

The editor reads that file to pick the next free signal and to warn about a
clash. It cannot read your code: a signal can sit behind a variable, as in
`band-app-arrows`, which sends `[0xbb, second]`. An app that skips the file is
invisible to the editor, and the next app will take its signal.

To see what is taken, open the editor, or read every `layout.json` and
`signals.json` under `apps/` and beside it.

Write the page in **ES5**: `var`, plain functions, no arrow functions, no
`const`, no template literals. Anything else blanks the page with no error.
Open BLE lazily on first press, never in `onInit`.

## Draw the icon and artwork

`scripts/make-band-assets.py` draws every asset for every app. Add a function
there rather than dropping a PNG in by hand, so a size or a color can be
changed once and regenerated.

- The app icon is **100 by 100**, a circular opaque background with the graphic
  inside 80 by 80 and a 2px transparent margin. That size comes from the device
  profile, not from the Zepp OS design guide, which covers round watches.
- Button artwork is the button's size, for example 180 by 202 for a stacked
  pill.

```sh
python3 scripts/make-band-assets.py
```

## Build and install

```sh
./scripts/build-band-apps.sh                       # add the new directory to it
```

`build-band-apps.sh` runs zmake, which converts the PNG files to the TGA
subtype the band reads. **Never zip the tree by hand**: a plain PNG under
`assets/` is ignored, and the app shows no icon with no error.

Then install `band-app-<name>/dist/band-app-<name>.bin` from the Mi Band Bridge
window: **Install or remove an app**, then **Install an app (.bin)**. With no
hand at the window, use the command line tool instead. With Mi Band Bridge open,
it sends the same install through Mi Band Bridge's socket (`docs/DEVELOPER.md`):

```sh
target/release/bandbridge install band-app-<name>/dist/band-app-<name>.bin
```

With **New app on top** on, the install puts the new app first in the band's
menu. With it off, the band puts the app where it likes: move it in **Arrange
what the device shows**.

## Give the new signal an action

In the Mi Band Bridge window, open **What each button does**, press **Add an
action**, press **Learn from the device**, and press the new button on the
band. Pick what it does, then press **Save**. The action counts from the next
press, with no reconnect.

Mi Band Bridge writes the action to
`~/Library/Application Support/BandBridge/config.toml`, like this:

```toml
[[actions]]
name = "Go to tab 5"
signal = "bb04"
keystroke = { key_code = "5", modifiers = ["command"] }
```

Or a media key instead, which takes no modifiers:

```toml
[[actions]]
name = "Play or pause"
signal = "bb04"
keystroke = { consumer_key_code = "play_or_pause" }
```

## Check it worked

Press the new button. The window's **Events from the device** shows a row with
`bb04` and the action's name. With Mi Band Bridge open, the command line tool
prints the same press in a terminal:

```sh
target/release/bandbridge follow      # prints "[app] bb04" and the action it fired
```

## Or use the editor

For an app of buttons that each send one fixed signal, the band app editor
does every step above with no code. See `docs/EDITOR.md`. Write an app by hand
only when it needs more than that.
