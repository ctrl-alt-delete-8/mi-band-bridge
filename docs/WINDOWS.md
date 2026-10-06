# Mi Band Bridge on Windows

Mi Band Bridge and the band app editor run on a Windows PC as two apps:

- **Mi Band Bridge** holds the band over Bluetooth and runs your actions. It has
  the same window as on a Mac, and an icon in the notification area in place
  of the menu bar item.
- **Mi Band App Editor** makes band apps of buttons, and installs them through
  Mi Band Bridge. See `docs/EDITOR.md`.

`README.md` describes the window, pairing and actions, and they are the same
on Windows, apart from what this page says. Both apps were built and tested on
Windows 11, x64.

## Install

Download `Mi-Band-Bridge-1.0.0-windows-x64-setup.exe` and
`Mi-Band-App-Editor-1.0.0-windows-x64-setup.exe` from the release page, as
Downloads in `README.md` says, or build them as "Build the installers" below
says.

Run each installer. It installs the app for your Windows user only, with no
administrator prompt, into `%LOCALAPPDATA%\Mi Band Bridge` or `%LOCALAPPDATA%\Mi
Band App Editor`, and adds it to the Start menu. Its last page offers a desktop
shortcut, and to start the app. A PC without WebView2 gets it from Microsoft
during the install.

The installers are not signed. The first time you run one that came from the
internet, Windows SmartScreen stops it with "Windows protected your PC". Click
**More info**, then **Run anyway**.

A newer installer replaces the app, and keeps your settings, your actions and
the band's key.

## Start Mi Band Bridge

Open Mi Band Bridge from the Start menu. It opens its window, and puts its icon
in the notification area. Windows may hide the icon under the ^ arrow. To keep
it in view, switch Mi Band Bridge on in Settings, Personalization, Taskbar,
Other system tray icons.

Click the icon to open the window. Right-click it for **Open Mi Band Bridge**
and **Quit**. Closing the window leaves Mi Band Bridge running, because it holds
the band. Opening Mi Band Bridge while it runs brings its window forward.

Mi Band Bridge does not start with Windows by itself. To start it when you sign
in, press Win+R, run `shell:startup`, and put a shortcut to
`%LOCALAPPDATA%\Mi Band Bridge\bandbridge-gui.exe` in the folder that opens.

## Pair your band

Pair the band as "Pair your band" in `README.md` says. The key goes into
Credential Manager, where a Mac uses the Keychain. Two things differ:

- Do not pair the band in Windows Settings. Windows would then hold the band,
  and Mi Band Bridge would not find it. Mi Band Bridge finds the band by a scan,
  and needs no Windows pairing.
- The band takes one host at a time. Quit Mi Band Bridge on your Mac first, and
  turn off your phone's Bluetooth if the phone holds the band. On Windows, the
  band is free as soon as Mi Band Bridge quits or disconnects, so there is no
  need to turn Bluetooth off and on.

## Actions on Windows

The window and `config.toml` are the same as on a Mac. These parts differ.

### Keystroke

The file keeps Karabiner's key names on Windows too, so it reads the same on
both systems. The window shows each modifier by its Windows name:

| In the file | On Windows | The window says |
|---|---|---|
| `command` | the Windows key | Win |
| `option` | Alt | Alt |
| `control` | Ctrl | Ctrl |
| `shift` | Shift | Shift |

So a config copied from a Mac presses Win where the Mac pressed command.
Change those actions, or record them again.

Mi Band Bridge sends the six media keys of a PC keyboard: volume up, volume
down, mute, play or pause, next track, and previous track. It also sends keys
that a Mac keyboard does not have: `insert`, `print_screen`, `scroll_lock`,
`pause`, `application` (the context menu key), and `f21` to `f24`. It cannot
send fn, brightness, keyboard lighting, Mission Control, Launchpad, rewind, fast
forward, eject, or the keys of Japanese keyboards. The window's list leaves
those out. A file that names one does not load, and the note says which key and
why.

**Record keys** hears the media keys, and shortcuts that Windows keeps for
itself, such as Win+Shift+S. A shortcut it records does nothing else: the
snipping tool does not open. Ctrl+Alt+Delete always goes to Windows.

**A keystroke cannot reach an app that runs as administrator**, such as Task
Manager, or a terminal opened with Run as administrator. Windows drops the keys
that Mi Band Bridge sends to it. Mi Band Bridge cannot tell, so the event log
shows no failure.

### Shell and Script

- **Shell** runs its line in Windows PowerShell, with no console window.
  Your PowerShell profile does not load.
- **Script** runs a file as a double-click runs it: a `.bat` or `.cmd` file,
  a program, a shortcut (`.lnk`), or a document, with its app. A `.ps1` file
  runs in PowerShell, where a double-click would open it in Notepad.
  PowerShell's execution policy does not stop it. A path that starts with `~`
  starts in your user folder.

### Shortcut

Shortcuts is a Mac app, so there is no Shortcut action on Windows. The action
editor has no Shortcut tab. A file that holds one does not load, and the note
says "Shortcuts are a Mac feature. Use a Script or Shell action on Windows."

### Web request

As on a Mac. HTTPS uses this PC's certificate store and its proxy settings.

## The band app editor

The editor works as `docs/EDITOR.md` says, with two differences.

Its build tools live in `%USERPROFILE%\.local\share\bandbridge-tools`, or in
the folder `BANDBRIDGE_TOOLS` names. When one is missing, the notice gives the
commands for PowerShell. They need Python 3 with the `py` launcher, and Git:

```powershell
py -3 -m venv "$env:USERPROFILE\.local\share\bandbridge-tools"
& "$env:USERPROFILE\.local\share\bandbridge-tools\Scripts\python.exe" -m pip install Pillow qrcode
git clone https://github.com/melianmiko/zmake "$env:USERPROFILE\.local\share\bandbridge-tools\zmake-src"
```

**Install on band** reaches Mi Band Bridge through a named pipe, where a Mac
uses a socket. Only your Windows user can open the pipe. It works while Mi Band
Bridge runs and its band is connected.

## Where Mi Band Bridge keeps things

| What | Where |
|---|---|
| the device, the settings and the actions | `%APPDATA%\BandBridge\config.toml` |
| the names of the apps installed from here | `%APPDATA%\BandBridge\app-names.toml` |
| the auth key | Credential Manager, Windows Credentials: a generic credential named for the band's MAC address, such as `00:00:5E:00:53:01.BandBridge` |
| the pipe that the band app editor installs through | `\\.\pipe\bandbridge-<SID>`, named by your Windows user's security ID |
| the two apps | `%LOCALAPPDATA%\Mi Band Bridge` and `%LOCALAPPDATA%\Mi Band App Editor` |

The settings folder, the key and the pipe keep BandBridge, the app's name
before version 1.0.0, so an update keeps your settings and the band's key.

The status bar names `%APPDATA%\BandBridge\config.toml`, with **Show in File
Explorer**.

## Uninstall

Uninstall each app from Settings, Apps, Installed apps. Mi Band Bridge's
uninstaller has a box, **Delete the application data**:

- Off, it keeps Mi Band Bridge's settings, its actions and the band's key.
  Install Mi Band Bridge again, and the band is paired as before.
- On, it also deletes `%APPDATA%\BandBridge` and the band's key. **Copy the
  key first**, with Copy key in the window: you need it to pair the band
  again.

In an upgrade, the box never deletes the settings or the key.

The editor keeps only the apps folder it opened. An uninstall forgets it. An
upgrade keeps it, unless you tick **Delete the application data**. Your band
apps stay where they are.

## Build the installers

Build both installers on a Windows PC with Rust and the Microsoft C++ Build
Tools, from a copy of this repository in a folder whose path has no space. In
Windows PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\build-windows.ps1
```

It writes `dist\windows\Mi-Band-Bridge-1.0.0-windows-x64-setup.exe` and
`dist\windows\Mi-Band-App-Editor-1.0.0-windows-x64-setup.exe`. The first run
also builds Tauri's command line tool into `target\tauri-cli`, which takes a
few minutes.

## When something fails

| What you see | What to do |
|---|---|
| "Windows protected your PC" when you run an installer | Click More info, then Run anyway. The installers are not signed. |
| "No band is nearby" when you pair | Wake the band, and keep it close. Quit Mi Band Bridge on your Mac, and turn off your phone's Bluetooth if the phone holds the band. If the band is paired in Windows Settings, remove it there. |
| "Bluetooth did not start within 20 seconds.", or "the scan did not start" | Turn Bluetooth on, in Settings, Bluetooth & devices. Then try again. |
| A keystroke does nothing in one app | That app runs as administrator, and Windows drops keys sent to it. |
| "Shortcuts are a Mac feature." | The file holds a Shortcut action, perhaps from a Mac. Use a Script or Shell action. |
| A note says Windows cannot send a key | Pick another key from the list in the window. |
