# Mi Band Bridge

Unofficial. Not made by or connected with Xiaomi.

**Mi Band Bridge** connects the buttons on a custom app (made by its companion software **Mi Band App Editor**, see [Make your own band app](#make-your-own-band-app)) of a Xiaomi Smart Band into actions on your computer (macOS or Windows): a keystroke, a script, a web request, or an Apple Shortcut (macOS only).

| App | What it does | Runs on |
|---|---|---|
| Mi Band Bridge | connect to the band via bluetooth, and runs the actions for each button press | Mac w/ Apple silicon, Windows |
| Mi Band App Editor | makes a band app consisting of buttons, w/ preview and no code | Mac w/ Apple silicon, Windows |
| Mi Band Bridge for Android | runs a Key Mapper key map for each button press, while Gadgetbridge helps connect the band | Android |

## Downloads

Every file is on the release page: [Here](https://github.com/ctrl-alt-delete-8/mi-band-bridge/releases/latest)

| File | What it is |
|---|---|
| `Mi-Band-Bridge-1.0.0-mac-arm64.dmg` | Mi Band Bridge and the Mi Band App Editor for a Mac w/ Apple silicon |
| `Mi-Band-Bridge-1.0.0-windows-x64-setup.exe` | Mi Band Bridge for Windows |
| `Mi-Band-App-Editor-1.0.0-windows-x64-setup.exe` | the Mi Band App Editor for Windows |
| `Mi-Band-Bridge-2.4.0-android.apk` | Mi Band Bridge for Android |
| `BandArrows-1.0.4.bin` | the sample band app, with an up and a down button |

## Install

### Mac

1. Open `Mi-Band-Bridge-1.0.0-mac-arm64.dmg`, and drag both apps onto the
   Applications folder in its window.
2. Open Mi Band Bridge from Applications. macOS asks once whether to open an
   app from the internet: choose Open. Then it asks whether Mi Band Bridge may
   use **Bluetooth**: allow it.

Mi Band Bridge lives in the menu bar when maintaining the connection to the band. 

Two more permissions come up when you first need them, and the window says
which one is missing:

- **Accessibility**, for a keystroke action. On macOS 27 the pane is called
  Device Control and Data Access.
- **Input Monitoring**, for **Record keys** to hear media keys and shortcuts
  that another app owns.

Mi Band Bridge keeps your band's key (see [Get the band's auth key](#get-the-bands-auth-key)) in your Mac's Keychain, where macOS stores
passwords. 

If an earlier build of Mi Band Bridge saved that key, macOS asks
once whether this build may read it. The dialog names the item `BandBridge`:
that is your band's key, saved when you paired. Choose **Always Allow**.

### Windows

Run each installer. It installs the app for your Windows user only. The installers are not signed, so Windows SmartScreen stops each one with "Windows protected your PC": click **More info**, then **Run anyway**.

### Android

Install `Mi-Band-Bridge-2.4.0-android.apk`. The first time, Android asks to let
your browser install apps. Mi Band Bridge for Android needs two other apps, both
on F-Droid:

- **[Gadgetbridge](https://codeberg.org/Freeyourgadget/Gadgetbridge)**, which connects to the band
- **[Key Mapper](https://github.com/keymapperorg/KeyMapper)**, which runs an action when a press lands

The app's Setup page links to both, and lists the steps that connect them.

## Get the band's auth key

Every band has its own auth key: 32 hex digits, which can look like a UUID.
You get it by logging in to the Zepp or Xiaomi account the band is paired with, for
example with huami-token. [Gadgetbridge's page](https://gadgetbridge.org/basics/pairing/huami-xiaomi-server/) lists the ways.

Keep the key somewhere safe. You need it again to pair the band on a different device.

## The sample band app, BandArrows

`BandArrows-1.0.4.bin` puts two buttons on the band's screen: up, which sends the signal
`bb02`, and down, which sends `bb03`. Install it:

- on a Mac or a Windows PC: in Mi Band Bridge's window, **Install or remove an
  app**, then **Install an app (.bin)**
- on Android: open the file with [Gadgetbridge's App installer](https://gadgetbridge.org/internals/features/installer/)

Then give each signal an action, such as Page Up and Page Down for slides or ebooks.

## Make your own band app

The **Mi Band App Editor** makes a band app of buttons. Its builds need Python 3, Pillow and zmake, and the editor shows the commands that install them.

The editor cannot yet make a button that switches between two states, a button that
counts, or a screen that shows something your computer sends. A band app written
by hand can, and an AI coding agent can write it for you:
[docs/BEYOND-THE-EDITOR.md](docs/BEYOND-THE-EDITOR.md) says how, and
[AGENTS.md](AGENTS.md) is the file to give your agent.

## Licenses

- The apps are free to use for all activities. You may share unchanged copies free of
  charge, and nobody may sell them: [LICENSE-APP.md](LICENSE-APP.md).
- The source code is under the PolyForm Noncommercial License 1.0.0:
  [LICENSE](LICENSE).
- The apps include software that others made, under their own licenses:
  [THIRD-PARTY-NOTICES](THIRD-PARTY-NOTICES).
