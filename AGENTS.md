# Band apps for Mi Band Bridge: notes for coding agents

A person can make a band app in the Mi Band App Editor: a page of buttons, each sending a signal that Mi Band Bridge turns into actions. The editor cannot make a button that keeps state, shows a count, or shows something the computer sends. This file is for the agent a person asks to do that. It holds what we learned on the device and what you will not find elsewhere. General JavaScript and Rust knowledge is assumed.

This file is the short entry. For depth, read `skills/zepp-os-app-build.md`, the platform's rules and the install protocol, and `skills/band-app.md`, this repository's recipe for a band app.

## The device

- **Xiaomi Smart Band 7 runs Zepp OS 1.0.** Use the 1.0 globals: `hmUI`, `hmBle`, `hmApp`, `hmSetting`, `timer`. The `@zos/*` modules and anything marked API level 2.0 or later do not run on it, and most examples online use them.
- **Write ES5 only:** `var`, `function`, no arrow functions, no `let` or `const`, no template literals. The build does not convert newer JavaScript. One syntax error shows a black screen with no message. Wrap every device call in `try`/`catch`, so a call the firmware lacks cannot blank the page.
- **The screen** is 192 × 490 px at 326 PPI, black. Its text font is Roboto: widths of Roboto Medium, drawn as thin as Roboto Regular. `text_size` is the em size. 25 px is the smallest size that reads easily at arm's length. Chinese characters are 1 em wide. `hmUI.getTextLayout(text, { text_size, text_width, wrapped })` measures text on the band.
- **Touch goes to the topmost widget.** A TEXT laid over a BUTTON blocks the button. A BUTTON's own `text` shows `\n` as a new line, and keeps its press color. A TEXT can take touches itself with `addEventListener(hmUI.event.CLICK_DOWN | CLICK_UP, fn)`.
- **Leave a bare strip at the bottom:** 70 px, as the editor and this repository's apps do, and never under 30 px. A back swipe that starts on a button presses it.
- **The screen turns off after 15 s.** While the app runs, `hmSetting.setBrightScreen(seconds)` keeps it on. Call `hmSetting.setBrightScreenCancel()` when the page is destroyed. `hmApp.setScreenKeep(true)` returns to the app when the band wakes.

## Talking to Mi Band Bridge

- **Send a signal:** call `hmBle.createConnect(function (index, data, size) {})` once, inside a `try`, on first use: a press, or a timer after the page is built. Never call it in `onInit` itself. Then check `hmBle.connectStatus() === true`, and call `hmBle.send(bytes.buffer, bytes.byteLength)` with `var bytes = new Uint8Array([0xbb, 0x05])`. Change the button's face only after `send` returns, since that is the only sign that the signal left the band.
- **The signal rule:** Mi Band Bridge acts on messages that start with `0xbb` followed by one to seven bytes, written in its config as hex (`bb05`). Never start a message with `0x01`, `0x02` or `0x03`: the band reads those as its own commands, and one of them manages and deletes apps.
- **Actions** live in Mi Band Bridge's window. One signal can run several actions. A person binds your signals there.
- **Declare every signal** the app sends in a `signals.json` beside its `app.json`, such as `{"signals": ["bb06", "bb07"]}`. The editor reads it to warn about two apps that send one signal. It cannot read your code.
- **The band holds one link at a time:** Mi Band Bridge on a Mac, Mi Band Bridge on Windows, or Gadgetbridge on Android, where Mi Band Bridge for Android hears it. Only the one that holds the band hears the presses.

## Building and installing

- **Start from a copy** of an app the editor generated (`<app folder>/generated/<name>/`), or of `band-app/` in this repository. Copy the generated one to a folder of its own beside the editor's apps first: the editor writes `generated/` again on every build. Give each app its own `appId` in `app.json`: a random number from 0xFFFF to 0x7FFFFFFF. Two apps with one id replace each other.
- **Build with zmake** (github.com/melianmiko/zmake), not Zepp's own tools. The editor's "Build tools are missing" notice gives the install commands. Then run zmake with the build tools' own Python, which has Pillow: `PYTHONPATH=~/.local/share/bandbridge-tools/zmake-src ~/.local/share/bandbridge-tools/bin/python -m zmake <app folder>`. On Windows that Python is `Scripts\python.exe` in the same folder. The package is `dist/<name>.bin`, named after the app's folder. zmake turns the PNGs under `assets/` into the format the band reads.
- **Install** in Mi Band Bridge's window: Install or remove an app, then Install an app (.bin). The band keeps no copy of a removed app.
- **Never print, log or commit the band's auth key.**

## Recipes

**A toggle.** Keep a variable on the page. Each press flips it, sends a different signal for each state (`bb06` for on, `bb07` for off), and repaints the button with `button.setProperty(hmUI.prop.MORE, { text: ..., normal_color: ..., press_color: ... })`. Mi Band Bridge then runs different actions for the two states. The state lives on the band, and resets when the app reopens.

**A count.** Keep a number on the page. Each press adds one and shows it in the button's text or a TEXT widget. Send a signal too, if the computer should act on it.

**A status from the computer.** Mi Band Bridge does not ship this, but it works. We proved it on a Band 7: up to 1000 bytes arrived whole, with a round trip of 61 to 180 ms.
- **Band side:** `hmBle.createConnect` takes a function that receives what the computer sends. Read it with `new Uint8Array(data)`, and show it in a TEXT widget.
- **Computer side:** Mi Band Bridge must write the bytes, unencrypted, to endpoint `0x00a0`, through the same session write that sends its app list request (`src/protocol/apps.rs`). Add a request for it to the socket server (`src/ipc.rs`), so a script can call it. Make that request put a fixed `0xbb` byte in front of whatever the caller gives, so a caller can never send the band's own command bytes.
