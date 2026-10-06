# The band app editor

Make a Xiaomi Smart Band 7 app of buttons, with no code. Each button sends one
fixed signal. For anything more, such as counters, toggles, or several pages,
write the app by hand. `docs/BEYOND-THE-EDITOR.md` explains how.

## What you need

- the editor, from the release page (see Downloads in `README.md`), or built
  from source with `cargo run -p bandbridge-editor`, which needs Rust and the
  Tauri 2 prerequisites for your platform
- the build tools: Python 3, Pillow, and zmake. See **Missing build tools**.
- to install from the editor: Mi Band Bridge open, with the band connected, on
  macOS or Windows. The editor installs through Mi Band Bridge. Otherwise,
  install the `.bin` on Android with Gadgetbridge's FW/App installer.
  Gadgetbridge takes a Zepp OS app as a zip with `app.json` at its root, which
  the `.bin` is. That install is not tested on a band yet.

The window follows the system's light or dark appearance.

## The window

From the top:

| Part | What it holds |
|---|---|
| Title bar | the editor's name, where Mi Band Bridge stands, and About |
| Toolbar | the apps folder, the app picker, New app, Build, and Install on band |
| Body | the start card until an app is open, then three panes: App, Buttons, and Preview |

### The title bar

The right end of the title bar says where Mi Band Bridge stands:

| It says | Meaning |
|---|---|
| Mi Band Bridge · band connected | Mi Band Bridge runs, and its band is connected |
| Mi Band Bridge · band not connected | Mi Band Bridge runs, and its band is not connected |
| Mi Band Bridge is not running | nothing answers on Mi Band Bridge's socket, or on its pipe on Windows |

The editor asks Mi Band Bridge every 5 seconds while its window is shown, and at
once when the window is shown again. Mi Band Bridge answers one request at a
time. While it installs an app, the title bar keeps what it said last.

The ⓘ button at the right end opens About: the editor's name and version, and
its licenses and notices.

### The toolbar

| Control | What it does |
|---|---|
| **Open folder…** | picks the folder that holds your apps. The folder shows beside it, and the editor opens it again next time. |
| **App** | the apps in the folder, by name, and how many there are. Pick one to open it. |
| **New app name**, **New app** | makes an app of that name in the folder, and opens it |
| **Build** | builds the open app. See **Build**. |
| **Install on band** | installs the open app's last build. See **Install through Mi Band Bridge**. |

An error in the checks turns off Build and Install on band, and "1 error stops
the build" shows before Build. Both are also off while a build or an install
runs.

### The start card

Until an app is open, the body shows **Open your apps folder**, with a second
**Open folder…** and the three steps: Design, Build, Install. A folder with no
apps keeps the card, and **New app name** takes the cursor.

### The App pane

The app's own settings, one to a row. Beside **App** is the app's file, such as
`slides/layout.json`.

| Setting | What it is |
|---|---|
| Name | the app's name on the band and in the picker |
| Color | the app's color. A new button starts in it. Without an icon picture, the app icon is a disc in this color. |
| App icon | the icon as the band gets it, with **Choose image…** and **Remove** |
| Columns | 1 or 2 |
| Button shape | Pill, Rounded, or Square |
| Free strip | the empty space at the bottom of the screen, from 0 to 484 px |

A color's swatch opens the system's color picker. The field beside it takes a
color as `#rrggbb`.

The checks are under the settings. See **The checks**.

### The Buttons pane

A row for each button, in the order the band draws them: its number, its color
or picture, its label, its signal, and buttons that move it up, move it down,
or remove it. The tags **Picture** and **Full width** mark those buttons.

Click a row to open it. One row is open at a time. The open row holds the
button's settings:

| Setting | What it is |
|---|---|
| Label | the words on the button |
| Signal | what the button sends. See **Signals**. |
| Color | the button's color |
| Pressed color | the color while the button is pressed. **Automatic** is a lighter shade of the button's color. **Choose** sets a color of your own. On a button with a picture, the margin takes this color, and the picture turns half way to it. |
| Picture | a PNG or JPEG file, with **Choose image…** and **Remove** |
| Full width | the button spans both columns. It works with 2 columns only. |

A button shows a label or a picture, never both. The band draws a label across
the middle of a button and cannot move it, so a button with a picture shows
the picture and not the label. The label stays in the file, and comes back
when you remove the picture.

The editor measures each label in Roboto Medium, whose widths the band's
labels have. A label fits its button when each of its lines is at least 8 px
narrower than the button, which leaves room for the rounded ends, and all its
lines together are no taller than the button. A line is 1.172 times the text
size tall, which holds the font's own space above and below the letters.

A label too wide for one line breaks at its spaces, onto as few lines as fit.
A word never breaks. The editor writes the breaks into the app, and the band
draws them.

All the labels of an app are one size: the largest, up to 34 px, at which
every label fits. The size never goes below 25 px, the smallest size that is
easy to read on the band (see **Sizes**). An app that allows smaller text goes
down to 10 px (see **The checks**). A label that does not fit even at the
smallest size gets a warning, and the band cuts it.

**Add a button** adds a button at the end and opens its row.

The Output drawer is at the foot of the pane. See **The Output drawer**.

### The Preview pane

The band's screen at 1.3 times its size, with the free strip marked. The
preview runs the same page as the band, so each button has the place, size and
color it has on the band. It draws each label at the app's size, on the
lines the band draws, and as thin as the band shows it, in Roboto Regular,
within 2% of the width the editor measures. While you press a
button, it shows the button's pressed color, or its pressed picture. Click a
button to see the signal it sends. Problems with the page show in red under
the screen, such as two buttons that send one signal.

## Make an app

1. Click **Open folder…**.
2. Pick the folder that holds your apps. An empty folder is fine.
3. Type a name in **New app name**.
4. Click **New app**.
5. Choose the columns and the button shape.
6. Click **Add a button** until the app has the buttons it needs.
7. In each button's row, set its label, its color, and a picture if you want
   one.
8. Click a button in the preview to see the signal it sends.
9. Read the checks, and fix each error.
10. Click **Build**.
11. Click **Install on band**.
12. In Mi Band Bridge, give each button an action. Open **What each button
    does**,
    press **Add an action**, press **Learn from the device**, then press the
    button on the band.

On Android, the relay learns a press in its own rule editor. See
`docs/ANDROID.md`.

## The checks

The editor saves and checks the app a moment after each change. The checks
show a card for each error and warning, with what is wrong and what to do.
**Show button** opens the row of the button a card is about.

An error stops a build. A warning is advice: **Dismiss** dims its card and
marks it "(dismissed)". A dismissed warning comes back when what it names
changes, such as the button's color. The gray card about smaller text shows
what that setting does. It is not advice, so it has no **Dismiss**, and the
counts leave it out.

The card about a label that does not fit at 25 px has **Allow smaller text**,
and the gray card has **Use 25 px**. These two buttons are the only way to
turn smaller text on and off. A dismissed card keeps its button.

| Finding | Kind |
|---|---|
| a color that is not `#rrggbb` | error |
| an app icon or a picture that is missing, outside the app folder, or not a PNG or JPEG file | error |
| a button with no signal, or with a signal that is not one | error |
| two or more buttons that send one signal | error |
| a button that has no room on the screen | error |
| a free strip under 30 px, where a back swipe may press a button | warning |
| a button under 5 mm wide or tall | warning |
| a label that does not fit its button, even at 25 px, or at 10 px when the app allows smaller text | warning |
| labels below 25 px, in an app that allows smaller text | setting |
| a signal that another app sends too | warning |
| a button color under 3 to 1 contrast with the black screen | warning |

The line under the cards says whether the app can be built: "No errors. Ready
to build.", "No errors. Built at 14:02." after a build until the next change,
or "Fix the error to build." while an error stands.

## Build

**Build** checks and saves the app, draws its buttons and pictures, and runs
zmake, which makes `dist/slides.bin` and `dist/slides.zip` for an app in
`slides/`. The Output drawer shows each step as it happens, with zmake's own
lines after "zmake:". While a build runs, the panes wait.

When zmake made the app, the app's version goes up by one. A build that stops
ends with a red line that says why, such as "zmake did not make slides.bin."
Above it, zmake's own lines say what went wrong. A zmake that runs for 2
minutes is stopped.

## The Output drawer

The drawer stays closed until the first build or install, and each one opens
it. Click **Output** to close or open it. Each line has its time. The drawer
keeps the lines of every build and install, from every app, until you click
**Clear**. The log is ten lines tall, and a line that wraps takes two or
more. Scroll up to read older lines.

The head of the drawer says what runs, or what stopped:

| The head says | When |
|---|---|
| Building… | a build runs |
| Installing… 62% | an install runs |
| Build stopped, Install stopped | the last build or install stopped. Its last line says why, or the card of an install that Mi Band Bridge could not take. |
| Build stopped: 1 error | an error in the checks stops the build |
| No build yet | nothing ran yet |

## Install through Mi Band Bridge

Mi Band Bridge holds the band, so the editor installs through it. **Install on
band** sends the app's `.bin` from `dist/` to Mi Band Bridge, and Mi Band Bridge
puts it on the band. Build the app first. While the install runs, the panes stay
in use.

Mi Band Bridge says how far the upload is. The button shows it as
"Installing… 62%", and one line of the drawer as
"Uploading to the band… 62% · 6.9 of 11.1 KB". An older Mi Band Bridge
does not send it, and the button reads "Installing…" until the install ends.

**Install on band** works only while the title bar says the band is connected,
and turns on and off with it, within 5 seconds, or at once after an install.
When Mi Band Bridge quits, or its band goes away, between that check and the
install, the install stops: the drawer's last line is "Install stopped.", and a
card says whether Mi Band Bridge is not running or its band is not connected,
with **Install again**. **Install again** works once the title bar says the band
is connected, and installs the app that stopped. Any other stop ends with a red
line that says why.

## Missing build tools

At start, the editor looks for Python 3, Pillow, and zmake in
`~/.local/share/bandbridge-tools`, or in the folder `BANDBRIDGE_TOOLS` names.
On Windows, the folder and the commands are in `docs/WINDOWS.md`.

When one is missing, the window shows **Build tools are missing** in place of
the start card and the panes, and every control in the toolbar is off. The
notice has:

- a row for each tool: **Found**, with its version or where it is, or
  **Missing**, with what it is for
- the commands that set up what is missing, on your system. **Copy** puts them
  on the clipboard.
- **Quit Mi Band App Editor**

Run the commands in a terminal, then open the editor again.

## The files

    apps/slides/
      layout.json   the app. The editor saves every change here.
      images/       picture files you added, PNG or JPEG
      generated/    written on every build. Do not edit.
      dist/         slides.bin and slides.zip

Commit `layout.json` and `images/`. Git ignores `generated/` and `dist/`.

## Signals

Each button sends one signal, from `bb01` to `bbff`. A signal that does not
start with `bb` is an error.

The reason is the band's own traffic. App signals and the band's platform
messages share one channel, and the first byte tells them apart: `01` is
JavaScript, `02` is app management, `03` is a screenshot. Starting every app at
`bb` keeps well clear, and 252 signals is more than a wrist has room for.

## Signals other apps use

The editor picks the next free signal, and warns when two apps send the same
one. To know what is taken it reads two kinds of file, one level under the
folder you opened and one level under the folder above it:

- `layout.json`, for an app the editor made.
- `signals.json`, for an app written by hand or by an LLM. It holds one line:
  `{"signals": ["bb04"]}`. The editor cannot read an app's code, because a
  signal can sit behind a variable, so an app that skips this file is invisible
  and the next app will take its signal.

An app installed on the band from somewhere else is invisible too. The band
reports an installed app as an id and nothing else, and it never sends back the
app itself, so no tool can list those signals. Press its buttons and watch the
events if you need to know.

## Sizes

The editor warns when a button is under 5 mm wide or tall, which is 64 px on
this band. 5 mm is the absolute minimum in Microsoft's touch guidance for a
phone, and it is not a measurement on this band. The recommended phone size,
7 mm, is not used: two columns give buttons 6.8 mm wide, so it would warn every
time you chose two columns.

Labels stay at 25 px or more unless an app allows smaller text. ISO 9241-303
and ANSI/HFES 100-2007 ask for capitals of at least 16 arcminutes. At 30 cm
from the eye, on this band's 326 PPI screen, that is 17.9 px. Roboto's capitals
are 0.711 of the text size, so that is a text size of 25.2 px, which the editor
takes as 25 px.

The screen is a capsule, with half circle ends, and the preview shows it that
way. A short button in the top row runs past that curve. The editor draws such
a button as an image trimmed to the curve, so its corners follow the screen and
the gap to the glass stays 6 px the whole way round. Every other button stays a
plain widget and carries no image. There is no warning for any of this, because
the preview shows it.
