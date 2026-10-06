// The window's behavior.
//
// This is a file, not an inline block: the content policy allows scripts
// from this app only, and an inline block is refused. Inline, none of this
// ran, which showed as an empty event list and buttons that did nothing.
// Reaching the Rust side must never throw at load. A failure here used to
// kill the whole script, which showed as an empty event list, a state stuck
// on "starting", and buttons that did nothing: three symptoms, one cause.
const bridge = window.__TAURI__ || {};
const invoke = bridge.core && bridge.core.invoke
  ? bridge.core.invoke
  : () => Promise.reject(new Error("the window cannot reach the app"));
const listen = bridge.event && bridge.event.listen
  ? bridge.event.listen
  : async () => {};
const log = document.getElementById("log");
const state = document.getElementById("state");
const pill = document.getElementById("pill");
const sections = document.querySelector("main");
// Whether the window runs on Windows, as system.js put it on the root.
// Windows has no Shortcut kind and no Bluetooth note, and the page draws its
// caption buttons. Everything else is the same on both systems.
const onWindows = document.documentElement.dataset.system === "windows";

// Which view the window shows: "setup" while no band is paired, "main" once
// one is, and "loading" until the page knows which.
let view = "loading";
document.body.dataset.view = view;
// True while a pair runs, from the press of Pair device to its answer.
let pairing = false;

// --- the event log ---------------------------------------------------------
// From the design frame "BandBridge Window — Light", section "Events from the
// device": a row for each press, with the action it fired and how that went,
// and a row for each app installed or removed, newest first. Nothing else
// goes in it. An action's failure shows in its own row, a connection problem
// on the connection card, and a pair's progress on the setup screen.
// events.js says what a row shows, and whether a record is new, a result for
// a row that is shown, or late for a row that is gone.
const live = document.getElementById("live");
// The rows on the page, by the id of their record.
const eventRows = new Map();
const ledger = eventLedger(500);

function logEvent(record) {
  const row = eventRow(record);
  if (!row) return;
  const step = ledger.take(record.id);
  if (step.act === "drop") return;
  const line = eventLine(row);
  if (step.act === "replace") {
    eventRows.get(record.id).replaceWith(line);
  } else {
    // A person reading further down keeps their place as rows arrive above.
    const scrolled = log.scrollTop;
    const tall = log.scrollHeight;
    log.insertBefore(line, step.before === null ? null : eventRows.get(step.before));
    if (scrolled > 0) log.scrollTop = scrolled + (log.scrollHeight - tall);
    for (const id of step.trim) {
      eventRows.get(id).remove();
      eventRows.delete(id);
    }
  }
  eventRows.set(record.id, line);
}

// The elements of one row, from what events.js says it shows.
function eventLine(row) {
  const line = document.createElement("div");
  line.className = "event";
  line.dataset.kind = row.kind;
  line.dataset.id = String(row.id);
  const time = document.createElement("time");
  time.className = "at";
  time.dateTime = row.at;
  time.textContent = row.time;
  line.append(time, piece("tag", row.tag));
  if (row.kind === "app") {
    line.append(piece("change", row.change), piece("app", row.app, row.app));
    return line;
  }
  line.append(piece("signal", row.signal));
  if (row.action) {
    line.append(lucide("arrow-right", "arrow"), piece("action", row.action, row.action));
  } else {
    line.append(piece("unset", row.unset));
  }
  if (row.running) {
    const running = piece("running", "", "Running");
    running.setAttribute("role", "img");
    running.setAttribute("aria-label", "running");
    running.append(lucide("loader"));
    line.append(running);
  } else if (row.result) {
    const chip = piece("result", "", row.result.title);
    chip.dataset.tone = row.result.tone;
    chip.append(lucide(row.result.mark), piece("words", row.result.words));
    line.append(chip);
  }
  return line;
}

// A span of words, with a tooltip when it has one.
function piece(name, words, tooltip) {
  const span = document.createElement("span");
  span.className = name;
  span.textContent = words;
  if (tooltip) span.title = tooltip;
  return span;
}

// An icon from the page's Lucide sprite.
function lucide(name, extra) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("class", extra ? `icon ${extra}` : "icon");
  svg.setAttribute("aria-hidden", "true");
  const use = document.createElementNS("http://www.w3.org/2000/svg", "use");
  use.setAttribute("href", `#i-${name}`);
  svg.append(use);
  return svg;
}

// Clear empties the log on this page, and nothing more. A result that comes
// later, for a row it took away, is dropped.
document.getElementById("clear").onclick = () => {
  ledger.clear();
  eventRows.clear();
  log.replaceChildren();
};

// Where the connection stands, as the band task last said: `state`
// (connected, connecting, retrying or disconnected), `reason` (started, you,
// dropped or failed), and `since`, the local time the state began. Every
// state line carries it. The page words it, as the design's connection cards
// do. Null until the band task first says.
let standing = null;
// True when this page cannot reach the app at all.
let unreachable = false;

// Live, in the log's head: the window hears the band's presses as they come
// while the band is connected. Otherwise the mark is dimmed.
function drawLive() {
  const on = view === "main" && !unreachable && standing !== null &&
    standing.state === "connected";
  live.dataset.live = String(on);
  live.title = on
    ? "Presses on the device show here as they come"
    : "The device is not connected, so no presses reach this window";
}

// The title bar's pill. The setup screen names its own state, as the design
// draws it. Everywhere else the pill says where the connection stands, in the
// design's words and tones.
function drawPill() {
  let words = "Starting";
  let shade = "idle";
  if (view === "setup") {
    words = pairing ? "Pairing" : "No device paired";
    shade = pairing ? "warn" : "unpaired";
  } else if (unreachable) {
    words = "No bridge";
    shade = "bad";
  } else if (standing) {
    ({ pill: words, pillTone: shade } = card(standing, null));
  }
  state.textContent = words;
  pill.dataset.tone = shade;
}

// A line from the band task. A state line moves the pill, the card and
// Live. Its radio word (ready, scanning, backoff) is for the terminal, and
// the page leaves it. An event line is a record for the event log. A pair's
// own lines go to the setup screen, which is all the window shows while a
// pair runs. An error line is a connection problem, which drain() weighs
// for the card. The band task sends the window nothing else.
//
// The window adds two lines as it starts. A config line says the config
// file could not be read, so no action fires until it is fixed: the note
// under the title bar says why, from the loader, as actions_list gives it.
// A permission line says an action posts keystrokes and macOS does not let
// this program post them: the banner asks again at once.
function show(line) {
  if (line.kind === "state") {
    if (line.connection) setStanding(line.connection);
  } else if (line.kind === "pair") {
    progress(line.text);
  } else if (line.kind === "event") {
    if (line.event) logEvent(line.event);
  } else if (line.kind === "config") {
    configFailedAtStart = true;
    loadActions();
  } else if (line.kind === "permission") {
    checkPermission();
  }
}

// Pushed lines would arrive here. The app gives the page no permission to
// listen, so none do, and the page collects its lines instead.
(async () => listen("band", (e) => show(e.payload)))().catch(() => {});

// The page collects whatever piled up. One collection runs at a time, so the
// lines are shown in the order they came: two answers in flight can land in
// either order, and an older state would then win. A line that cannot be
// shown is a fault in this page. It is reported, and the rest go on.
let draining = Promise.resolve();
function drain() {
  draining = draining
    .then(async () => {
      const lines = await invoke("drain_lines");
      for (const line of lines) {
        try {
          show(line);
        } catch (e) {
          console.error("a line from the band task was not shown", line, e);
        }
      }
      // Weighed once the whole batch is in: the state line that goes with a
      // problem comes after it.
      const problem = lines.filter((line) => line.kind === "error").pop();
      if (problem) connectionProblem(problem.text);
    })
    .catch(() => { /* the band task is not up yet */ });
  return draining;
}
setInterval(drain, 500);
// Once at load too, so what the window said as it started, such as a
// config file that does not load, shows with the first view.
drain();

// A connection problem: why a connect failed, or could not begin. The card
// shows where that left the band. A few problems need their own words under
// it as well: a Connect with no key to connect with, a paired band whose key
// is not in the store, another band that answered. So a problem shows under
// the card when it leaves the band idle, unless the link dropped, which the
// card names itself. A problem the band task tries again past goes by, as
// the card says it is trying again.
let unplaced = null;
function connectionProblem(text) {
  if (!standing) {
    // It came before the page knew where the connection stands, which
    // setStanding() tells it.
    unplaced = text;
    return;
  }
  if (view === "setup" || standing.state !== "disconnected" || standing.reason === "dropped") {
    return;
  }
  setNote(sentence(text), true);
}

// --- the connection card ----------------------------------------------------
// From the design frame "State variations — Connection": the band's name, its
// MAC, Copy key, a line that says where the connection stands and since when,
// and the buttons for that state. connection.js holds the words. The id this
// Mac uses to reach the band stays in config.toml, and the window never shows
// it (D2).
const deviceName = document.getElementById("device-name");
const deviceMac = document.getElementById("device-mac");
const copyKey = document.getElementById("copy-key");
const standingLine = document.getElementById("standing");
const standingText = document.getElementById("standing-text");
const note = document.getElementById("connection-note");
const buttons = {
  connect: document.getElementById("connect"),
  disconnect: document.getElementById("disconnect"),
  cancel: document.getElementById("cancel-connect"),
  "stop-trying": document.getElementById("stop-trying"),
  forget: document.getElementById("forget"),
};
// The paired band as the config names it, {name, mac}, and the `device`
// answer: the name, the MAC the band gave on its last link, and the battery.
// The config's copy fills in until the band has been reached.
let paired = null;
let band = null;

function drawConnection() {
  const name = (band && band.name) || (paired && paired.name) || "";
  const mac = (band && band.mac) || (paired && paired.mac) || "";
  deviceName.textContent = name;
  deviceName.title = name;
  deviceMac.textContent = mac;
  deviceMac.hidden = !mac;
  // Copy key and Forget act on the paired band, whose key is kept under its
  // MAC. A card with no MAC knows no band: config.toml could not be read as
  // the app started. It offers neither, until the file is fixed and read.
  copyKey.hidden = !mac;
  const shown = view === "main" && standing ? card(standing, band && band.battery) : null;
  standingLine.dataset.tone = shown ? shown.tone : "dim";
  standingLine.dataset.icon = shown ? shown.icon : "";
  standingLine.querySelector("use").setAttribute("href", `#i-${shown ? shown.icon : "circle-slash"}`);
  standingLine.querySelector(".icon").style.visibility = shown ? "" : "hidden";
  standingText.textContent = shown ? shown.text : "\u00a0";
  const focused = document.activeElement;
  for (const [id, button] of Object.entries(buttons)) {
    button.hidden = !(shown && shown.buttons.includes(id)) || (id === "forget" && !mac);
  }
  buttons.connect.textContent = (shown && shown.connect) || "Connect";
  // A button that goes away under the keyboard's focus hands it to the
  // button that takes its place: Connect after Disconnect, and so on.
  if (focused && focused.hidden && Object.values(buttons).includes(focused)) {
    const next = Object.values(buttons).find((button) => !button.hidden);
    if (next) next.focus();
  }
}

function setStanding(next) {
  const before = standing;
  standing = next;
  // The note under the card is for a band that was let go.
  if (next.state !== "disconnected") setNote(null);
  drawPill();
  drawConnection();
  drawLive();
  if (!before && unplaced) {
    const text = unplaced;
    unplaced = null;
    connectionProblem(text);
  }
  if (!before || before.state !== next.state) {
    followConnection();
    refreshDevice();
    // The band reports its battery a moment after the link comes up.
    if (next.state === "connected") setTimeout(refreshDevice, 2500);
  }
}

// A line under the card: advice, or why a copy failed.
function setNote(text, bad = false) {
  if (note.hidden && !text) return;
  note.textContent = text || "";
  note.hidden = !text;
  note.classList.toggle("bad", bad);
  fitHeight();
}

// What the band task knows about the band. The `device` request also asks
// the band for its battery, for the next answer, so the page asks when the
// state changes and once a minute, and never twice at once.
let askingDevice = false;
async function refreshDevice() {
  if (askingDevice || view !== "main") return;
  askingDevice = true;
  try {
    band = JSON.parse(await invoke("device"));
    drawConnection();
  } catch (e) {
    /* the band task is not up yet: the next change or minute asks again */
  } finally {
    askingDevice = false;
  }
}
setInterval(refreshDevice, 60000);

// The paired band as the config names it, read again after a pair.
function loadPaired() {
  invoke("saved_setup")
    .then((text) => {
      paired = JSON.parse(text).device;
      drawConnection();
    })
    .catch(() => {});
}

// Each button returns at once, and the change arrives as a state line, so
// the page collects it soon rather than at the next half second. A press
// the band task could not take is a connection problem, said under the card.
function press(command) {
  setNote(null);
  invoke(command)
    .then(() => setTimeout(drain, 150))
    .catch((e) => setNote(sentence(String(e)), true));
}
// Letting the band go is not enough on its own: the Mac keeps the band as a
// paired device, and the band talks to one at a time, so a phone still cannot
// reach it. Disconnect says so, and so does Forget. Windows lets the band go
// with BandBridge, so it has no such note.
const LET_GO_NOTE = onWindows ? null : "Turn Bluetooth on this Mac off, then back on, so it " +
  "lets go of the band. Otherwise your phone cannot connect to it.";
buttons.connect.onclick = () => press("connect");
buttons.disconnect.onclick = () => {
  press("disconnect");
  if (LET_GO_NOTE) setNote(LET_GO_NOTE);
};
// Cancel lets go as Disconnect does. The connect runs to its end, and lets
// the band go when it lands.
buttons.cancel.onclick = () => press("disconnect");
buttons["stop-trying"].onclick = () => press("stop_trying");

// --- the key --------------------------------------------------------------
// Copy key asks Rust to read the key from the system's secret store and to
// write it to the clipboard. The key never enters this page. The page hears
// only whether the copy worked, and says where the key went. `fail` shows why
// a copy failed, and clears that when given null. Returns a function that
// puts the button back as it was.
function copyKeyButton(button, fail) {
  const label = button.querySelector("span");
  const mark = button.querySelector("use");
  const idle = { words: label.textContent, icon: mark.getAttribute("href") };
  let timer = 0;
  const draw = (copied) => {
    button.classList.toggle("copied", copied);
    label.textContent = copied ? "Key on the clipboard" : idle.words;
    mark.setAttribute("href", copied ? "#i-check" : idle.icon);
  };
  button.onclick = async () => {
    clearTimeout(timer);
    draw(false);
    fail(null);
    button.disabled = true;
    try {
      await invoke("copy_key");
      draw(true);
      timer = setTimeout(() => draw(false), 4000);
    } catch (e) {
      fail(String(e));
    } finally {
      button.disabled = false;
    }
  };
  return () => {
    clearTimeout(timer);
    draw(false);
  };
}

// On the card, a failure shows under it, and a new try clears only that.
copyKeyButton(copyKey, (text) => {
  if (text) setNote(sentence(text), true);
  else if (note.classList.contains("bad")) setNote(null);
});

// A line under a section: why something it asked for did not happen, or,
// not in red, what it did. The event log holds events only, so a section
// that asks for something says how it went itself.
function say(note, text, bad = true) {
  if (note.hidden && !text) return;
  note.textContent = text ? sentence(text) : "";
  note.hidden = !text;
  note.classList.toggle("bad", Boolean(text) && bad);
  fitHeight();
}

// --- install or remove an app -------------------------------------------------
// From the design frame "BandBridge Window — Light", section "Install or
// remove an app": the apps installed from here, each with its id and version
// and Remove, and Install an app (.bin) under them. The band names an app by
// its id alone, so the list holds the apps this Mac can name, which are the
// ones it installed. A removal asks first, in the row: the band keeps no
// copy. An install releases the band, sends the app, and connects again.
// An app that went on or off is a row in the event log, from the band task.
// arrange.js holds the words. The band's lists are read while its section is
// open, and again each time the band connects.
const appsSection = document.getElementById("apps");
const appList = document.getElementById("app-list");
const appsNote = document.getElementById("apps-note");
const installButton = document.getElementById("install");
const installStatus = document.getElementById("install-status");
// The apps as apps_list last answered, or null while they are not known.
let apps = null;
// While the first read runs, and why the last read failed.
let appsLoading = false;
let appsFailed = null;
let appsAsked = 0;
// The app whose row asks before a removal, and the one being removed.
let appAsked = null;
let appRemoving = null;
// The install that runs or last ended, {stage, path, error}, or null.
let installing = null;

// Whether the band is there to be read and changed.
function connectedNow() {
  return view === "main" && !unreachable && standing !== null && standing.state === "connected";
}

// Where the connection stands, for a list that waits for the band.
function bandAway() {
  if (standing === null) return "unknown";
  return standing.state === "connecting" || standing.state === "retrying" ? "coming" : "away";
}

function installRunning() {
  return installing !== null && installing.stage === "running";
}

// Whether the band task can read or change a list now. During an install it
// is busy with the app, and the band connects again after it, when the open
// lists are read.
function bandFree() {
  return connectedNow() && !installRunning();
}

async function loadApps() {
  if (!connectedNow()) {
    apps = null;
    appsLoading = false;
    appsFailed = null;
    appAsked = null;
    drawApps();
    return;
  }
  if (!bandFree()) return;
  const asked = ++appsAsked;
  appsLoading = apps === null;
  appsFailed = null;
  drawApps();
  try {
    const answer = JSON.parse(await invoke("apps_list"));
    if (asked !== appsAsked) return;
    apps = answer.apps;
  } catch (e) {
    if (asked !== appsAsked) return;
    apps = null;
    appsFailed = String(e);
  }
  appsLoading = false;
  drawApps();
}

// What a list's box says when it has no rows, with a way to try again
// after a failure.
function listState(words, tone, retry) {
  const line = document.createElement("div");
  line.className = tone ? `list-state ${tone}` : "list-state";
  if (tone === "loading") line.append(lucide("loader"));
  line.append(piece("", words));
  if (retry) {
    const again = document.createElement("button");
    again.type = "button";
    again.className = "link";
    again.textContent = "Try again";
    again.onclick = retry;
    line.append(again);
  }
  return line;
}

function drawApps() {
  let rows;
  if (apps !== null) {
    rows = apps.length === 0 ? [listState(appsEmptyWords())] : apps.map(appLine);
  } else if (appsFailed !== null) {
    rows = [listState(sentence(`the apps could not be read: ${reasonOf(appsFailed)}`), "bad", loadApps)];
  } else if (appsLoading || bandAway() === "unknown") {
    rows = [listState("Reading the apps on the device…", "loading")];
  } else {
    rows = [listState(awayWords(bandAway(), "its apps"))];
  }
  appList.replaceChildren(...rows);
  drawInstall();
}

// One app: its name, its id and version, and Remove, or the question
// before a removal, or the removal that runs.
function appLine(app) {
  const words = appRow(app);
  const line = document.createElement("div");
  line.className = "app-row";
  line.setAttribute("role", "listitem");
  line.dataset.id = app.id;
  if (appAsked === app.id) return askRemove(line, app);
  const badge = document.createElement("span");
  badge.className = "app-badge";
  badge.append(lucide("box"));
  const meta = document.createElement("span");
  meta.className = "app-meta";
  meta.append(piece("mono", words.id));
  if (words.version) meta.append(piece("", "·"), piece("mono", words.version));
  const stack = document.createElement("div");
  stack.className = "stack";
  stack.append(piece("app-name", words.name, words.name), meta);
  line.append(badge, stack);
  if (appRemoving === app.id) {
    line.classList.add("removing");
    const working = piece("working", "");
    working.append(lucide("loader"), piece("", "Removing…"));
    line.append(working);
    return line;
  }
  const remove = document.createElement("button");
  remove.type = "button";
  remove.className = "remove";
  remove.append(lucide("trash-2"), piece("", "Remove"));
  remove.setAttribute("aria-label", `Remove ${words.name}`);
  remove.disabled = appRemoving !== null || installRunning();
  remove.onclick = () => {
    appAsked = app.id;
    say(appsNote, null);
    drawApps();
    const keep = appList.querySelector(".app-row.confirming button");
    if (keep) keep.focus();
  };
  line.append(remove);
  return line;
}

// The question before a removal, in the row: the band keeps no copy.
function askRemove(line, app) {
  const keep = document.createElement("button");
  keep.type = "button";
  keep.textContent = "Cancel";
  keep.onclick = () => {
    appAsked = null;
    drawApps();
  };
  const remove = document.createElement("button");
  remove.type = "button";
  remove.className = "danger";
  remove.textContent = "Remove";
  remove.onclick = () => removeApp(app);
  line.classList.add("confirming");
  line.replaceChildren(piece("confirm-words", removeQuestion(app)), keep, remove);
  return line;
}

async function removeApp(app) {
  appAsked = null;
  appRemoving = app.id;
  say(appsNote, null);
  drawApps();
  try {
    await invoke("apps_remove", { id: app.id });
    appRemoving = null;
    // Gone at once, and the band's list then says what it holds.
    if (apps !== null) apps = apps.filter((listed) => listed.id !== app.id);
    drawApps();
    await loadApps();
    // The app leaves the band's menu too.
    if (arrangeSection.open) loadColumn(columns.menu);
  } catch (e) {
    appRemoving = null;
    say(appsNote, `removing ${appRow(app).name} did not work: ${reasonOf(e)}`);
    drawApps();
  }
}

// Install an app (.bin): the picker, then the install, which says how it
// goes beside the button. The band connects again after it, and its lists
// are read then.
function drawInstall() {
  const running = installRunning();
  installButton.disabled = running || !connectedNow() || appRemoving !== null;
  installButton.setAttribute("aria-busy", String(running));
  installButton.querySelector("span").textContent = running ? "Installing…" : "Install an app (.bin)";
  installButton.title = connectedNow() || running ? "" : "Connect the device to install an app";
  installStatus.hidden = installing === null;
  if (installing === null) return;
  const said = installWords(installing.stage, installing.path, installing.error);
  installStatus.dataset.tone = said.tone;
  installStatus.querySelector("use").setAttribute("href",
    `#i-${said.tone === "busy" ? "loader" : said.tone === "good" ? "check" : "triangle-alert"}`);
  installStatus.querySelector("span").textContent = said.words;
}

installButton.onclick = async () => {
  if (installRunning()) return;
  say(appsNote, null);
  let path;
  try {
    path = await invoke("pick_app");
  } catch (e) {
    say(appsNote, `the file picker did not open: ${e}`);
    return;
  }
  if (!path) return;
  installing = { stage: "running", path };
  drawApps();
  try {
    await invoke("install", { file: path });
    installing = { stage: "done", path };
  } catch (e) {
    installing = { stage: "failed", path, error: String(e) };
  }
  drawApps();
};

appsSection.addEventListener("toggle", () => {
  if (appsSection.open) loadApps();
});

// --- arrange what the device shows ------------------------------------------
// From the same frame, section "Arrange what the device shows": the app menu
// and the shortcuts, side by side, each as the band shows it, and under a
// bar that folds, what it keeps out of view. A row is dragged by any part of
// it, or moved with the keyboard: Space picks it up, the arrow keys move
// it, and Space puts it down. A drop changes the band at once, and the list
// is then read back from the band. The control center is not in the window
// (D5). arrange.js says what each row shows, and whether a move can be made.
const arrangeSection = document.getElementById("arrange");
const arrangeNote = document.getElementById("arrange-note");
const arrangeLive = document.getElementById("arrange-live");
const columns = Object.fromEntries(["menu", "shortcuts"].map((list) => [list, {
  list,
  box: document.getElementById(`${list}-screens`),
  count: document.getElementById(`${list}-count`),
  // The list as the band last gave it, or null while it is not known.
  entries: null,
  loading: false,
  failed: null,
  // A move is being written to the band.
  busy: false,
  // The hidden group is open. It starts folded: the band keeps many
  // screens out of view.
  hiddenOpen: false,
  asked: 0,
}]));
// The row being moved, with the pointer or the keyboard, or null.
let drag = null;

function loadColumns() {
  for (const col of Object.values(columns)) loadColumn(col);
}

async function loadColumn(col) {
  if (!connectedNow()) {
    col.entries = null;
    col.loading = false;
    col.failed = null;
    drawColumn(col);
    return;
  }
  if (!bandFree()) return;
  const asked = ++col.asked;
  col.loading = col.entries === null;
  col.failed = null;
  drawColumn(col);
  let entries = null;
  let failed = null;
  try {
    entries = JSON.parse(await invoke("screens_list", { list: col.list }));
  } catch (e) {
    failed = String(e);
  }
  if (asked !== col.asked) return;
  col.entries = entries;
  col.failed = failed;
  col.loading = false;
  drawColumn(col);
}

function drawColumn(col) {
  // A row being moved keeps its place until it is put down, and the list
  // is drawn then.
  if (drag && drag.col === col) {
    col.stale = true;
    return;
  }
  col.stale = false;
  col.box.setAttribute("aria-busy", String(col.busy));
  col.count.textContent = col.busy ? "Saving…" : col.entries ? listCounts(col.entries) : "";
  let rows;
  if (col.entries !== null) {
    const shown = shownEntries(col.entries).map((entry) => screenRow(col, entry));
    if (shown.length === 0) shown.push(listState("Nothing shown. Drag a screen here."));
    const hidden = hiddenEntries(col.entries);
    rows = [...shown, hiddenBar(col, hidden.length),
            ...(col.hiddenOpen ? hidden.map((entry) => screenRow(col, entry)) : [])];
  } else if (col.failed !== null) {
    rows = [listState(sentence(`${LISTS[col.list].name} could not be read: ${reasonOf(col.failed)}`),
                      "bad", () => loadColumn(col))];
  } else if (col.loading || bandAway() === "unknown") {
    rows = [listState("Reading the device…", "loading")];
  } else {
    rows = [listState(awayWords(bandAway(), "this list"))];
  }
  col.box.replaceChildren(...rows);
}

// One screen: the grip, its icon, its words, and the custom tag for an app
// installed from here.
function screenRow(col, entry) {
  const row = document.createElement("div");
  row.className = entry.shown ? "screen" : "screen off";
  row.setAttribute("role", "listitem");
  row.tabIndex = col.busy ? -1 : 0;
  row.dataset.id = entry.id;
  row.title = screenTitle(entry);
  row.setAttribute("aria-label", entry.shown ? screenLabel(entry) : `${screenLabel(entry)}, hidden`);
  row.setAttribute("aria-describedby", "arrange-how");
  const badge = document.createElement("span");
  badge.className = "screen-badge";
  badge.append(lucide(screenIcon(entry)));
  row.append(lucide("grip-vertical", "grip"), badge, piece("screen-name", screenLabel(entry)));
  if (isCustom(entry)) row.append(piece("custom", "custom"));
  return row;
}

// The bar over what the band keeps out of view: it folds the group, counts
// it, and takes a row dropped on it.
function hiddenBar(col, count) {
  const bar = document.createElement("button");
  bar.type = "button";
  bar.className = "hidden-bar";
  bar.setAttribute("aria-expanded", String(col.hiddenOpen));
  bar.append(lucide(col.hiddenOpen ? "chevron-down" : "chevron-right", "fold"), lucide("eye-off", "eye"),
             piece("word", "Hidden"), piece("n", String(count)), piece("rule", ""));
  bar.onclick = () => {
    col.hiddenOpen = !col.hiddenOpen;
    drawColumn(col);
    const again = col.box.querySelector(".hidden-bar");
    if (again) again.focus();
  };
  return bar;
}

// The shown rows other than the one being moved, top to bottom.
function otherShown(col) {
  return [...col.box.querySelectorAll(".screen:not(.off)")].filter((row) => row !== drag.row);
}

// Where the pointer would put the row being moved: a place among the other
// shown rows, or the hidden group, below the middle of its bar.
function placeAt(col, y) {
  const rows = otherShown(col);
  for (let at = 0; at < rows.length; at += 1) {
    const box = rows[at].getBoundingClientRect();
    if (y < box.top + box.height / 2) return at;
  }
  const bar = col.box.querySelector(".hidden-bar").getBoundingClientRect();
  return y < bar.top + bar.height / 2 ? rows.length : "hidden";
}

// Show where the row would land: a line between two rows, or the hidden
// group's bar lit, in red when the band would not take the move.
function showPlace(col, to) {
  drag.to = to;
  const refused = Boolean((moveTo(col.list, col.entries, drag.id, to) || {}).refused);
  const bar = col.box.querySelector(".hidden-bar");
  bar.classList.toggle("target", to === "hidden");
  bar.classList.toggle("refused", to === "hidden" && refused);
  let line = col.box.querySelector(".drop-line");
  if (to === "hidden") {
    if (line) line.remove();
    return;
  }
  if (!line) {
    line = document.createElement("div");
    line.className = "drop-line";
    col.box.append(line);
  }
  line.classList.toggle("refused", refused);
  const rows = otherShown(col);
  const edge = to < rows.length
    ? rows[to].getBoundingClientRect().top
    : bar.getBoundingClientRect().top;
  // The box's border is a point wide, and the line sits in the point
  // between two rows.
  line.style.top = `${edge - col.box.getBoundingClientRect().top - 1 - 1.5}px`;
}

// A row taken to be moved. With the keyboard it lifts at once, and with the
// pointer once the pointer moves.
function pickUp(col, row, how) {
  drag = { col, row, id: row.dataset.id, to: placeOf(col.entries, row.dataset.id), moved: false, ...how };
}

function lift() {
  drag.moved = true;
  drag.row.classList.add("dragging");
  drag.col.box.classList.add("sorting");
}

function putDown() {
  if (!drag) return null;
  const { col, row, id, to, moved } = drag;
  row.classList.remove("dragging");
  row.style.transform = "";
  col.box.classList.remove("sorting");
  const line = col.box.querySelector(".drop-line");
  if (line) line.remove();
  const bar = col.box.querySelector(".hidden-bar");
  if (bar) bar.classList.remove("target", "refused");
  drag = null;
  if (col.stale) drawColumn(col);
  return { col, id, to, moved };
}

for (const col of Object.values(columns)) {
  const box = col.box;
  box.addEventListener("pointerdown", (event) => {
    const row = event.target.closest(".screen");
    if (!row || event.button !== 0 || drag || col.busy || col.entries === null || !bandFree()) return;
    // No text is selected, and the row keeps the keyboard's focus.
    event.preventDefault();
    row.focus();
    pickUp(col, row, { pointer: event.pointerId, startY: event.clientY });
    // The row follows the pointer past the box's edge. A pointer the
    // browser does not track cannot be held, and the drag works without.
    try {
      row.setPointerCapture(event.pointerId);
    } catch (e) {
      /* the moves still come while the pointer is over the box */
    }
  });
  box.addEventListener("pointermove", (event) => {
    if (!drag || drag.col !== col || drag.pointer !== event.pointerId) return;
    const dy = event.clientY - drag.startY;
    if (!drag.moved) {
      // A press that barely moves is a click.
      if (Math.abs(dy) < 4) return;
      lift();
    }
    drag.row.style.transform = `translateY(${dy}px)`;
    showPlace(col, placeAt(col, event.clientY));
  });
  const release = (event) => {
    if (!drag || drag.col !== col || drag.pointer !== event.pointerId) return;
    const put = putDown();
    if (put.moved && event.type === "pointerup") applyMove(col, put.id, put.to, false);
  };
  box.addEventListener("pointerup", release);
  box.addEventListener("pointercancel", release);
  box.addEventListener("keydown", (event) => moveByKey(col, event));
  // A row picked up with the keyboard goes back if the focus leaves it.
  box.addEventListener("focusout", (event) => {
    if (drag && drag.col === col && drag.pointer === undefined && event.relatedTarget !== drag.row) {
      putDown();
      arrangeLive.textContent = "Moved nothing.";
    }
  });
}

// Space picks a row up, the arrow keys move it one place, Space or Enter
// puts it down, and Escape puts it back.
function moveByKey(col, event) {
  const row = event.target.closest(".screen");
  if (!drag) {
    if (!row || (event.key !== " " && event.key !== "Enter") || col.busy || col.entries === null ||
        !bandFree()) return;
    event.preventDefault();
    pickUp(col, row, {});
    lift();
    showPlace(col, drag.to);
    arrangeLive.textContent = placeWords(col.list, col.entries, drag.id, drag.to);
    return;
  }
  if (drag.col !== col || drag.pointer !== undefined) return;
  if (event.key === "ArrowUp" || event.key === "ArrowDown") {
    event.preventDefault();
    const places = placesFor(col.entries, drag.id);
    const at = places.indexOf(drag.to) + (event.key === "ArrowUp" ? -1 : 1);
    showPlace(col, places[Math.max(0, Math.min(places.length - 1, at))]);
    arrangeLive.textContent = placeWords(col.list, col.entries, drag.id, drag.to);
  } else if (event.key === " " || event.key === "Enter") {
    event.preventDefault();
    const put = putDown();
    applyMove(col, put.id, put.to, true);
  } else if (event.key === "Escape") {
    event.preventDefault();
    putDown();
    arrangeLive.textContent = "Moved nothing.";
  }
}

// Write a move to the band: the list shows the move at once, the band takes
// it, and the list is then read back from the band, which has the last word.
async function applyMove(col, id, to, byKey) {
  const verdict = col.entries === null ? null : moveTo(col.list, col.entries, id, to);
  if (verdict === null) {
    if (byKey) focusScreen(col, id);
    return;
  }
  if (verdict.refused) {
    say(arrangeNote, verdict.refused);
    if (byKey) focusScreen(col, id);
    return;
  }
  say(arrangeNote, null);
  const before = col.entries;
  col.entries = arranged(col.entries, verdict.shown);
  col.busy = true;
  // A read on its way is older than this move.
  col.asked += 1;
  drawColumn(col);
  let written = false;
  try {
    await invoke("screens_apply", { list: col.list, shown: verdict.shown });
    written = true;
    col.entries = JSON.parse(await invoke("screens_list", { list: col.list }));
    if (!showsAsAsked(col.entries, verdict.shown)) {
      say(arrangeNote, `the device did not take that order, so ${LISTS[col.list].name} shows what it holds.`);
    }
  } catch (e) {
    if (written) {
      say(arrangeNote, `the device took the new order, but ${LISTS[col.list].name} could not be read back: ${reasonOf(e)}`);
    } else {
      col.entries = before;
      say(arrangeNote, `${LISTS[col.list].name} did not change: ${reasonOf(e)}`);
    }
  }
  col.busy = false;
  // A band that went away meanwhile shows no list.
  if (!connectedNow()) col.entries = null;
  drawColumn(col);
  if (byKey) focusScreen(col, id);
  arrangeLive.textContent = col.entries ? listCounts(col.entries) : "";
}

function focusScreen(col, id) {
  const row = col.box.querySelector(`.screen[data-id="${id}"]`);
  if (row) row.focus();
}

arrangeSection.addEventListener("toggle", () => {
  if (arrangeSection.open) {
    loadColumns();
  } else if (drag) {
    putDown();
  }
});

// The lists are the band's. Each is read when its section opens, and again
// when the band connects. While the band is away, none is shown.
function followConnection() {
  if (connectedNow()) {
    if (appsSection.open) loadApps();
    if (arrangeSection.open) loadColumns();
    drawInstall();
    return;
  }
  if (drag) putDown();
  appsAsked += 1;
  apps = null;
  appsLoading = false;
  appsFailed = null;
  appAsked = null;
  drawApps();
  for (const col of Object.values(columns)) {
    col.asked += 1;
    col.entries = null;
    col.loading = false;
    col.failed = null;
    drawColumn(col);
  }
}
drawApps();
for (const col of Object.values(columns)) drawColumn(col);

// --- settings ---------------------------------------------------------------
// From the design frame "BandBridge Window — Light": Auto reconnect, New app
// on top, and Appearance. A change is written to config.toml at once, and
// takes effect with no restart: the band task follows the file, and the
// window takes Appearance itself. The page uses the file's names for them.
const autoReconnect = document.getElementById("auto-reconnect");
const newAppOnTop = document.getElementById("new-app-on-top");
const appearanceChoices = [...document.querySelectorAll('input[name="appearance"]')];
const settingsNote = document.getElementById("settings-note");
// The settings as the file holds them, or null while they are not known.
let settings = null;
// One read or write at a time, in the order asked, so an older answer never
// lands after a newer one. `unwritten` counts the changes not yet answered.
let settingsQueue = Promise.resolve();
let unwritten = 0;

function drawSettings() {
  const known = settings !== null;
  for (const input of [autoReconnect, newAppOnTop, ...appearanceChoices]) input.disabled = !known;
  if (!known) return;
  autoReconnect.checked = settings.auto_reconnect;
  newAppOnTop.checked = settings.new_app_on_top;
  for (const choice of appearanceChoices) choice.checked = choice.value === settings.appearance;
  // The page's theme: a class on the root forces light or dark, and none
  // follows the system. The window's own theme follows the same setting.
  const root = document.documentElement;
  root.classList.toggle("light", settings.appearance === "light");
  root.classList.toggle("dark", settings.appearance === "dark");
}

// Why the settings could not be read or saved, under them.
function setSettingsNote(text) {
  if (settingsNote.hidden && !text) return;
  settingsNote.textContent = text ? sentence(text) : "";
  settingsNote.hidden = !text;
  fitHeight();
}

// Read again after a pair or a forget, which can change the file.
function loadSettings() {
  settingsQueue = settingsQueue.then(async () => {
    try {
      settings = await invoke("settings_get");
      setSettingsNote(null);
    } catch (e) {
      settings = null;
      // A file that does not load is said once, in the note under the
      // title bar.
      setSettingsNote(isConfigError(String(e))
        ? "the settings show again once config.toml is fixed."
        : `the settings could not be read: ${e}`);
    }
    drawSettings();
  });
}

// Shown at once, then written, and put right by what the file then holds.
function changeSetting(change) {
  settings = { ...settings, ...change };
  drawSettings();
  unwritten += 1;
  settingsQueue = settingsQueue.then(async () => {
    try {
      const saved = await invoke("settings_set", { change });
      // With a newer change waiting, its answer is the one that stands.
      if (unwritten === 1) settings = saved;
      setSettingsNote(null);
    } catch (e) {
      setSettingsNote(`the change was not saved: ${e}`);
      try {
        settings = await invoke("settings_get");
      } catch (_) {
        settings = null;
      }
    } finally {
      unwritten -= 1;
      drawSettings();
    }
  });
}

autoReconnect.addEventListener("change", () =>
  changeSetting({ auto_reconnect: autoReconnect.checked }));
newAppOnTop.addEventListener("change", () =>
  changeSetting({ new_app_on_top: newAppOnTop.checked }));
for (const choice of appearanceChoices) {
  choice.addEventListener("change", () => {
    if (choice.checked) changeSetting({ appearance: choice.value });
  });
}
loadSettings();

// --- the actions ------------------------------------------------------------
// From the design frame "BandBridge Window — Light", section "What each
// button does", and the frame "State variations — Action kinds" for the
// editor's four tabs. The list is config.toml as actions_list reads it. The
// editor sends one action back through actions_save, which checks it by the
// file's rules and names the field of each problem, and the problem shows
// beside that field. actions.js holds the words. Every command here is the
// window's alone (D9).
const actionList = document.getElementById("action-list");
const actionCount = document.getElementById("action-count");
const actionsNote = document.getElementById("actions-note");
const addAction = document.getElementById("add-action");
// What actions_list last answered, or null while the file is not read.
let actionsListed = null;
// The names a keystroke can use, from key_names, or null until it answers.
let keyNames = null;
// The list was read at least once: the band holds what the file had then.
let actionsReadOnce = false;
// Reads are counted, so an answer older than the one shown is dropped. A
// save or a delete answers the list too, and is newer than any read.
let actionsAsked = 0;
let actionsShown = 0;

function loadActions() {
  const asked = ++actionsAsked;
  return invoke("actions_list")
    .then((answer) => {
      if (asked < actionsShown) return;
      actionsShown = asked;
      takeActions(answer);
      showConfigBroken(null);
    })
    .catch((e) => {
      if (asked < actionsShown) return;
      actionsShown = asked;
      const text = String(e);
      if (isConfigError(text)) {
        actionsListed = null;
        showConfigBroken(text);
        drawActions();
      } else {
        say(actionsNote, `the actions could not be read: ${text}`);
      }
    });
}

// A list the file holds, from a read, a save or a delete.
function takeActions(answer) {
  actionsListed = answer;
  actionsReadOnce = true;
  configFailedAtStart = false;
  // Asking for the input permission is worth a banner only when an action
  // posts a keystroke. A shell command, script, shortcut, or web request
  // needs nothing.
  postsKeys = answer.postsKeys;
  drawActions();
  checkPermission();
}

function drawActions() {
  let rows;
  if (actionsListed === null) {
    rows = configBroken ? [emptyRow("The actions show again once config.toml is fixed.")] : [];
  } else if (actionsListed.actions.length === 0) {
    rows = [emptyRow("No actions yet. Add one, then press its button on the device.")];
  } else {
    rows = actionsListed.actions.map((item, at) => actionRow(item, at));
  }
  actionList.replaceChildren(...rows);
  actionList.hidden = rows.length === 0;
  actionCount.hidden = actionsListed === null;
  actionCount.textContent = actionsListed ? countWords(actionsListed.actions.length) : "";
  addAction.disabled = actionsListed === null || draft !== null;
}

// One action: its signal, its name, its kind and what it does, and Edit and
// Delete. The row the editor changes is marked, and while the editor is
// open, no other row can be edited or deleted. Rows are told apart by their
// place in the list, since two actions may share a signal (D6).
function actionRow(item, at) {
  const words = listRow(item, keyNames);
  const line = document.createElement("div");
  line.className = "action-row";
  line.setAttribute("role", "listitem");
  line.dataset.signal = item.action.signal;
  const mech = document.createElement("div");
  mech.className = "mech";
  mech.append(piece("kind-word", words.kind),
              piece(words.mono ? "value mono" : "value", words.value, words.title));
  const stack = document.createElement("div");
  stack.className = "stack";
  stack.append(piece("action-name", words.name, words.name), mech);
  const edit = iconButton("pencil-line", `Edit ${words.name}`);
  edit.onclick = () => openEditor(item, at);
  const remove = iconButton("x", `Delete ${words.name}`);
  remove.onclick = () => askDelete(line, item, at);
  edit.disabled = remove.disabled = draft !== null;
  const buttons = document.createElement("div");
  buttons.className = "row-buttons";
  buttons.append(edit, remove);
  line.append(piece("sig", words.signal), stack, buttons);
  line.classList.toggle("editing", draft !== null && draft.was !== null && draft.was.at === at);
  return line;
}

function emptyRow(text) {
  const line = document.createElement("div");
  line.className = "action-row empty";
  line.textContent = text;
  return line;
}

function iconButton(icon, label) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "icon-button";
  button.setAttribute("aria-label", label);
  button.title = label;
  button.append(lucide(icon));
  return button;
}

// Delete asks first, in the row: the file is written at once, and a press
// of the signal then does nothing, or runs only the other actions on it.
function askDelete(line, item, at) {
  const { name, signal } = item.action;
  const others = actionsListed.actions.filter((other, place) =>
    place !== at && other.action.signal.toLowerCase() === signal.toLowerCase()).length;
  const then = others === 0
    ? `Then a press of ${signal} does nothing.`
    : `A press of ${signal} still runs the ${others === 1 ? "other action" : `other ${others} actions`} on it.`;
  const keep = document.createElement("button");
  keep.type = "button";
  keep.textContent = "Cancel";
  keep.onclick = () => drawActions();
  const remove = document.createElement("button");
  remove.type = "button";
  remove.className = "danger";
  remove.textContent = "Delete";
  remove.onclick = async () => {
    keep.disabled = remove.disabled = true;
    try {
      const answer = await invoke("actions_delete", { was: { at, name, signal } });
      actionsShown = ++actionsAsked;
      say(actionsNote, null);
      takeActions(answer);
    } catch (e) {
      say(actionsNote, `${name} was not deleted: ${e}`);
      drawActions();
    }
  };
  line.classList.add("confirming");
  line.replaceChildren(
    piece("confirm-words", `Delete “${name}”? ${then}`),
    keep, remove);
  keep.focus();
}

// --- config.toml that does not load (item 7) --------------------------------
// The loader's reason names the action and the field, so a file edited by
// hand can be fixed. It shows in one place, the note under the title bar,
// with Reveal in Finder. Settings and the card leave it to the note. While
// the file does not load, the page looks again every two seconds, so a fix
// made in another app shows here at once.
const configBanner = document.getElementById("config-banner");
// Why config.toml does not load, as the loader says it, or null.
let configBroken = null;
// The file did not load as the app started, so no action fires.
let configFailedAtStart = false;
let configLook = 0;

function showConfigBroken(text) {
  const was = configBroken;
  configBroken = text ? configReason(text) : null;
  configBanner.hidden = configBroken === null;
  document.getElementById("config-consequence").textContent =
    configBroken ? configConsequence(configFailedAtStart, actionsReadOnce) : "";
  document.getElementById("config-reason").textContent = configBroken || "";
  clearInterval(configLook);
  if (configBroken) configLook = setInterval(loadActions, 2000);
  if (was !== null && configBroken === null) {
    // Fixed: what else reads the file reads it again.
    loadSettings();
    loadPaired();
  }
  if ((was === null) !== (configBroken === null)) fitHeight();
}
document.getElementById("config-reveal").onclick = () => {
  invoke("reveal_config").catch((e) => statusSays(String(e)));
};

// A person who fixed the file elsewhere comes back to the window.
window.addEventListener("focus", () => {
  if (view === "main") loadActions();
});

// --- the editor -----------------------------------------------------------
const editor = document.getElementById("editor");
const editorDivider = document.getElementById("editor-divider");
const editorForm = document.getElementById("editor-form");
const nameField = document.getElementById("action-name");
const signalField = document.getElementById("action-signal");
const learnButton = document.getElementById("learn");
const learnHint = document.getElementById("learn-hint");
const tabButtons = [...editor.querySelectorAll('[role="tab"]')];
const tabBodies = [...editor.querySelectorAll("[data-body]")];
const capsBox = document.getElementById("caps");
const recordButton = document.getElementById("record-keys");
const recordNoteLine = document.getElementById("record-note");
const keyPick = document.getElementById("key-pick");
const modifierChips = [...editor.querySelectorAll("[data-modifier]")];
const keysWarning = document.getElementById("keys-warning");
const keysWarningWords = document.getElementById("keys-warning-words");
const commandField = document.getElementById("action-command");
const scriptField = document.getElementById("action-script");
const shortcutPick = document.getElementById("action-shortcut");
const shortcutsSaid = document.getElementById("shortcuts-said");
const shortcutsRefresh = document.getElementById("shortcuts-refresh");
const methodPick = document.getElementById("action-method");
const urlField = document.getElementById("action-url");
const headersField = document.getElementById("action-headers");
const bodyField = document.getElementById("action-body");
const saveButton = document.getElementById("action-save");
const tryButton = document.getElementById("action-try");
const tryNote = document.getElementById("try-note");
const tryResult = document.getElementById("try-result");
// The action in the editor: `was`, the signal it had when the list was read,
// or null for a new one, and `fields`, as actions.js reads them. Null while
// the editor is closed.
let draft = null;

addAction.onclick = () => openEditor(null);
document.getElementById("action-cancel").onclick = closeEditor;
document.getElementById("editor-close").onclick = closeEditor;

// One editor at a time, under the list, for a new action or the row marked
// above it.
function openEditor(item, at) {
  if (draft) return;
  const action = item ? item.action : null;
  // The action being edited, as the list showed it: two may share a signal.
  const was = action ? { at, name: action.name, signal: action.signal } : null;
  draft = { was, fields: editorState(action) };
  const fields = draft.fields;
  document.getElementById("editor-title").textContent = action ? "Edit action" : "New action";
  nameField.value = fields.name;
  signalField.value = fields.signal;
  commandField.value = fields.command;
  scriptField.value = fields.script;
  methodPick.value = fields.method;
  urlField.value = fields.url;
  headersField.value = fields.headers;
  bodyField.value = fields.body;
  noteAfterRecording = false;
  clearProblems();
  showTry(null);
  editor.hidden = false;
  editorDivider.hidden = false;
  drawActions();
  drawEditor();
  sizeSignal();
  fitTextareas();
  if (fields.tab === "shortcut" && shortcutNames === null) loadShortcuts();
  editor.scrollIntoView({ block: "nearest" });
  nameField.focus();
}

// Cancel, or the close mark. Nothing is saved, and a learn, a recording or
// a countdown stops with the editor.
function closeEditor() {
  if (!draft) return;
  const focused = editor.contains(document.activeElement);
  stopRecording();
  stopLearning();
  stopCountdown();
  draft = null;
  editor.hidden = true;
  editorDivider.hidden = true;
  drawActions();
  if (focused) addAction.focus();
}

function drawEditor() {
  if (!draft) return;
  const tab = draft.fields.tab;
  for (const button of tabButtons) {
    const chosen = button.dataset.tab === tab;
    button.setAttribute("aria-selected", String(chosen));
    button.tabIndex = chosen ? 0 : -1;
  }
  for (const body of tabBodies) body.hidden = body.dataset.body !== tab;
  drawKeystroke();
  drawShell();
  drawShortcuts();
  drawLearn();
  drawTryButton();
}

// Shortcuts are a Mac feature (W5): on Windows the editor has no Shortcut
// tab. The Shell tab's examples are Windows' own there: a PowerShell line,
// and a PowerShell script.
if (onWindows) {
  document.getElementById("tab-shortcut").hidden = true;
  commandField.placeholder = "Start-Process notepad";
  scriptField.placeholder = "~\\scripts\\toggle-desk-lamp.ps1";
}

// The tab strip stays put, and only the fields below it change. Each tab
// keeps what was typed in it.
for (const button of tabButtons) {
  button.onclick = () => chooseTab(button.dataset.tab);
  button.addEventListener("keydown", (event) => {
    const step = { ArrowRight: 1, ArrowLeft: -1 }[event.key];
    if (!step) return;
    event.preventDefault();
    const shown = tabButtons.filter((tab) => !tab.hidden);
    const at = shown.indexOf(button);
    const next = shown[(at + step + shown.length) % shown.length];
    chooseTab(next.dataset.tab);
    next.focus();
  });
}

function chooseTab(tab) {
  if (!draft || draft.fields.tab === tab) return;
  stopRecording();
  stopCountdown();
  draft.fields.tab = tab;
  clearProblems();
  showTry(null);
  drawEditor();
  fitTextareas();
  if (tab === "shortcut" && shortcutNames === null && !askingShortcuts) loadShortcuts();
}

nameField.addEventListener("input", () => {
  draft.fields.name = nameField.value;
  clearProblem("name");
});
signalField.addEventListener("input", () => {
  draft.fields.signal = signalField.value;
  clearProblem("signal");
  sizeSignal();
});

// The signal field is as wide as the signal in it.
function sizeSignal() {
  signalField.style.width = `${Math.max(6, signalField.value.length + 1)}ch`;
}

// --- a problem, beside its field --------------------------------------------
// actions_save and action_try answer every problem at once, each named by
// its field. A problem whose field is not in view, or that is no field's,
// shows under Save.
function showProblem(slot, words) {
  const own = editor.querySelector(`[data-error="${slot}"]`);
  const inView = own && !own.parentElement.closest("[hidden]");
  const line = inView ? own : editor.querySelector('[data-error="general"]');
  line.textContent = line.hidden || !line.textContent ? words : `${line.textContent} ${words}`;
  line.hidden = false;
  const field = editor.querySelector(`[data-slot="${slot}"]`);
  if (field && inView) field.classList.add("invalid");
}

function clearProblem(slot) {
  const line = editor.querySelector(`[data-error="${slot}"]`);
  if (line) {
    line.hidden = true;
    line.textContent = "";
  }
  const field = editor.querySelector(`[data-slot="${slot}"]`);
  if (field) field.classList.remove("invalid");
}

function clearProblems() {
  for (const line of editor.querySelectorAll("[data-error]")) {
    line.hidden = true;
    line.textContent = "";
  }
  for (const field of editor.querySelectorAll(".invalid")) field.classList.remove("invalid");
}

function showProblems(refused) {
  clearProblems();
  for (const [slot, words] of problemsBySlot(refused)) showProblem(slot, words);
  const first = editor.querySelector(".field-error:not([hidden])");
  if (first) first.scrollIntoView({ block: "nearest" });
}

// --- Learn from the device ---------------------------------------------------
// learn answers the next press's signal, and the band task fires nothing for
// it. A second press of the button stops the learn, and so does closing the
// editor: a learn left waiting would take a press a person meant to fire.
let learning = null;
learnButton.onclick = () => (learning ? stopLearning() : startLearning());

async function startLearning() {
  clearProblem("learn");
  const mine = {};
  learning = mine;
  drawLearn();
  try {
    const signal = await invoke("learn");
    if (learning !== mine || !draft || !signal) return;
    draft.fields.signal = signal;
    signalField.value = signal;
    sizeSignal();
    clearProblem("signal");
  } catch (e) {
    if (learning === mine) showProblem("learn", sentence(String(e)));
  } finally {
    if (learning === mine) {
      learning = null;
      drawLearn();
    }
  }
}

function stopLearning() {
  if (!learning) return;
  learning = null;
  invoke("learn_stop").catch(() => {});
  drawLearn();
}

function drawLearn() {
  const waiting = learning !== null;
  learnButton.setAttribute("aria-pressed", String(waiting));
  learnButton.querySelector("span").textContent =
    waiting ? "Waiting for a press…" : "Learn from the device";
  learnHint.textContent = waiting
    ? "Press the button on the device. Press here again to stop."
    : "Press a button on the device that you want to use.";
}

// --- Keystroke: Record keys and the key list ---------------------------------
// Record keys asks the app for the next key a person presses. The Mac's event
// tap and the Windows keyboard hook both hear the keys another app owns and
// the media keys, and keep the key they record from acting (B-009b). When the
// tap or the hook cannot hear, the app says so, and this page listens itself:
// it hears only the keys that reach the window. The key list offers every key
// and every media key, always (D3).
// A recording that runs: `by` is "tap" or "page", `modifiers` is true once
// a modifier went down, and `ticket` is its number, which its stop names, so
// a stop that reaches the app before the recording began still ends it. The
// numbers start at the time the page loaded, so no page reuses another's.
let recording = null;
let recordTickets = Date.now();
// The note under Record keys stays up after the sign that another program
// took the keys: modifiers, then the window lost the focus (D7).
let noteAfterRecording = false;

recordButton.onclick = () => (recording ? stopRecording() : startRecording());

async function startRecording() {
  if (!draft) return;
  clearProblem("keys");
  noteAfterRecording = false;
  // The app's tap on the Mac, and its hook on Windows, hear the keys. Each
  // says so if it cannot, and the page takes over.
  const mine = { by: "tap", modifiers: false, ticket: ++recordTickets };
  recording = mine;
  drawKeystroke();
  let answer;
  try {
    answer = await invoke("record_keys", { ticket: mine.ticket });
  } catch (e) {
    if (recording !== mine) return;
    recording = null;
    showProblem("keys", sentence(String(e)));
    drawKeystroke();
    return;
  }
  if (recording !== mine) return;
  if (answer && answer.page) {
    // The tap cannot hear: the page listens from here.
    mine.by = "page";
    drawKeystroke();
    return;
  }
  recording = null;
  if (answer && answer.heard && draft) heardKeys(answer.heard);
  drawKeystroke();
}

// Stop the recording that runs, from every path: a second press of Record
// keys, Cancel, the close mark, another tab, a folded section, or the window
// losing the focus. It always tells the app to stop the recording its ticket
// names, whether the app's hook is listening ("tap") or the page took over
// ("page"), so no hook or tap is left in. A new recording starts clean.
function stopRecording() {
  if (!recording) return;
  invoke("record_keys_stop", { ticket: recording.ticket }).catch(() => {});
  recording = null;
  drawKeystroke();
}

function heardKeys(keystroke) {
  draft.fields.keystroke = keystroke;
  clearProblem("keys");
}

// The page's own recorder: a key's place, from its keydown's code, and the
// four flags. It takes the keys first, so none of them acts on the page.
window.addEventListener("keydown", (event) => {
  if (!recording) return;
  const heard = pageKeystroke(event, keyNames);
  if (heard.modifier) recording.modifiers = true;
  if (recording.by !== "page") return;
  event.preventDefault();
  event.stopPropagation();
  if (heard.modifier || event.repeat) return;
  const ticket = recording.ticket;
  recording = null;
  // The page took over because the app said it could not hear. The app's wait
  // has already ended, so this stops nothing, but it names the ticket in case
  // a hook or a tap is still waiting.
  invoke("record_keys_stop", { ticket }).catch(() => {});
  if (heard.keystroke) {
    heardKeys(heard.keystroke);
  } else {
    showProblem("keys", `This window has no name for that key (${heard.unnamed || "no code"}). ` +
      "Pick it from the list.");
  }
  drawKeystroke();
}, true);

// Modifiers, then the window lost the focus: another program took the
// shortcut. The recording stops, and the note says at once what to do.
window.addEventListener("blur", () => {
  if (!recording) return;
  const taken = recording.modifiers;
  stopRecording();
  if (taken) {
    noteAfterRecording = true;
    drawRecordNote();
  }
});

function drawKeystroke() {
  if (!draft) return;
  const keystroke = draft.fields.keystroke;
  const listening = recording !== null;
  capsBox.classList.toggle("listening", listening);
  const shown = caps(keystroke, keyNames);
  if (listening) capsBox.replaceChildren(piece("none", "Press the keys…"));
  else if (shown.length === 0) capsBox.replaceChildren(piece("none", "No keys yet"));
  else capsBox.replaceChildren(...shown.map((cap) => piece(cap.mark ? "cap mark" : "cap", cap.label, cap.title)));
  capsBox.title = keysWords(keystroke, keyNames);
  keyPick.value = pickedValue(keystroke, keyNames);
  keyPick.classList.toggle("empty", keyPick.value === "");
  const media = Boolean(keystroke && keystroke.consumer_key_code);
  const held = new Set(keystroke && !media ? modifiersHeld(keystroke.modifiers, keyNames) : []);
  for (const chip of modifierChips) {
    chip.setAttribute("aria-pressed", String(held.has(chip.dataset.modifier)));
    chip.disabled = media;
    chip.title = media ? "A media key takes no modifiers" : "";
  }
  recordButton.setAttribute("aria-pressed", String(listening));
  recordButton.querySelector("span").textContent = listening ? "Recording…" : "Record keys";
  drawRecordNote();
}

// The note under Record keys (D7): while a recording runs, and after the
// sign that another program took the keys. Its words follow the input
// permission.
function drawRecordNote() {
  recordNoteLine.hidden = !(recording !== null || noteAfterRecording);
  recordNoteLine.textContent = recordNote(inputAllowed !== false);
  recordNoteLine.classList.toggle("after", noteAfterRecording && recording === null);
  keysWarning.hidden = inputAllowed !== false;
}

// The key list, once key_names answers: every key and every media key, in
// groups, by the names the file takes.
let askingKeyNames = false;
function loadKeyNames() {
  if (keyNames || askingKeyNames) return;
  askingKeyNames = true;
  invoke("key_names")
    .then((names) => {
      keyNames = names;
      const groups = keyGroups(names).map((group) => {
        const options = document.createElement("optgroup");
        options.label = group.label;
        options.append(...group.names.map((name) => new Option(
          group.kind === "key" ? modifierKeyLabel(name, names) : name,
          `${group.kind}:${name}`,
        )));
        return options;
      });
      keyPick.replaceChildren(new Option("Pick a key", ""), ...groups);
      drawModifierChips(names);
      drawActions();
      drawKeystroke();
    })
    .catch(() => { /* the next look asks again */ })
    .finally(() => { askingKeyNames = false; });
}

// The modifier toggles, as the key list names them: the Mac's marks and
// names, or on Windows the labels key_names gives, Ctrl, Alt, Shift and Win
// (W2). A modifier this system cannot hold, fn on Windows, has no toggle. On
// a PC the chips also read in the Windows order, Ctrl, Alt, Shift, Win, as
// the caps and the words do.
function drawModifierChips(names) {
  for (const chip of modifierChips) {
    const modifier = chip.dataset.modifier;
    chip.hidden = !names.modifiers.includes(modifier);
    const label = names.labels && names.labels[modifier];
    if (label) chip.textContent = label;
  }
  if (pcKeyboard(names)) {
    const row = modifierChips[0].parentNode;
    for (const modifier of WINDOWS_MODIFIER_ORDER) {
      const chip = modifierChips.find((c) => c.dataset.modifier === modifier);
      if (chip) row.append(chip);
    }
  }
}

keyPick.onchange = () => {
  if (!draft || !keyPick.value) return;
  const [kind, name] = [keyPick.value.slice(0, keyPick.value.indexOf(":")),
                        keyPick.value.slice(keyPick.value.indexOf(":") + 1)];
  draft.fields.keystroke = kind === "media"
    ? withMediaKey(name)
    : withKey(draft.fields.keystroke, name, keyNames);
  clearProblem("keys");
  noteAfterRecording = false;
  drawKeystroke();
};

for (const chip of modifierChips) {
  chip.onclick = () => {
    if (!draft) return;
    draft.fields.keystroke = toggleModifier(draft.fields.keystroke, chip.dataset.modifier, keyNames);
    clearProblem("keys");
    drawKeystroke();
  };
}

// Without the input permission, a keystroke goes nowhere. macOS's own dialog
// adds BandBridge to the list it honors, and opens the pane from there.
document.getElementById("keys-open-settings").onclick = async () => {
  try {
    const allowed = await invoke("request_input_permission");
    inputAllowed = allowed;
    banner.hidden = !postsKeys || allowed;
    if (!allowed) {
      keysWarningWords.textContent = "Allow Mi Band Bridge in System Settings, Privacy & Security, " +
        "Device Control and Data Access, then reopen this app.";
    }
    drawRecordNote();
  } catch (e) {
    keysWarningWords.textContent = sentence(String(e));
  }
};

// --- Shell: a command, or a script file --------------------------------------
// The one set last is the one the action runs. The other stays as typed,
// dimmed, and is not sent.
commandField.addEventListener("input", () => {
  draft.fields.command = commandField.value;
  draft.fields.shellMode = "shell";
  clearProblem("command");
  drawShell();
});
scriptField.addEventListener("input", () => {
  draft.fields.script = scriptField.value;
  draft.fields.shellMode = "script";
  clearProblem("script");
  drawShell();
});

document.getElementById("choose-script").onclick = async () => {
  if (!draft) return;
  try {
    const path = await invoke("pick_script");
    if (!path || !draft) return;
    draft.fields.script = path;
    draft.fields.shellMode = "script";
    scriptField.value = path;
    clearProblem("script");
    drawShell();
  } catch (e) {
    showProblem("script", sentence(String(e)));
  }
};

function drawShell() {
  if (!draft) return;
  for (const part of editor.querySelectorAll("[data-alt]")) {
    part.classList.toggle("unused", part.dataset.alt !== draft.fields.shellMode);
  }
}

// --- Shortcut: the picker, with Refresh ---------------------------------------
// What shortcuts_list answered, or null until it has, and why it could not.
let shortcutNames = null;
let shortcutsFailed = null;
let askingShortcuts = false;

async function loadShortcuts() {
  askingShortcuts = true;
  drawShortcuts();
  try {
    shortcutNames = await invoke("shortcuts_list");
    shortcutsFailed = null;
  } catch (e) {
    shortcutsFailed = String(e);
  } finally {
    askingShortcuts = false;
    drawShortcuts();
  }
}
shortcutsRefresh.onclick = () => {
  if (!askingShortcuts) loadShortcuts();
};
shortcutPick.onchange = () => {
  if (!draft) return;
  draft.fields.shortcut = shortcutPick.value;
  clearProblem("shortcut");
  drawShortcuts();
};

// The shortcut an action runs stays in the picker when Shortcuts no longer
// has it, and says so.
function drawShortcuts() {
  if (!draft) return;
  const chosen = draft.fields.shortcut;
  const names = shortcutNames || [];
  const options = chosen ? [] : [new Option("Pick a shortcut", "")];
  options.push(...names.map((name) => new Option(name, name)));
  if (chosen && !names.includes(chosen)) {
    options.push(new Option(shortcutNames ? `${chosen} (not in Shortcuts)` : chosen, chosen));
  }
  shortcutPick.replaceChildren(...options);
  shortcutPick.value = chosen;
  let said = "";
  if (askingShortcuts) said = "Asking Shortcuts…";
  else if (shortcutsFailed) said = sentence(shortcutsFailed);
  else if (shortcutNames) said = shortcutsWords(shortcutNames.length);
  shortcutsSaid.textContent = said;
  shortcutsSaid.classList.toggle("bad", !askingShortcuts && shortcutsFailed !== null);
  shortcutsRefresh.disabled = askingShortcuts;
}

// --- Web request: method, address, headers and body --------------------------
methodPick.onchange = () => {
  draft.fields.method = methodPick.value;
  clearProblem("method");
};
urlField.addEventListener("input", () => {
  draft.fields.url = urlField.value;
  clearProblem("url");
});
headersField.addEventListener("input", () => {
  draft.fields.headers = headersField.value;
  clearProblem("headers");
  fitTextareas();
});
bodyField.addEventListener("input", () => {
  draft.fields.body = bodyField.value;
  fitTextareas();
});

// The header and the body take one line, and grow with more.
function fitTextareas() {
  for (const area of [headersField, bodyField]) {
    area.style.height = "15px";
    if (area.scrollHeight > 15) area.style.height = `${area.scrollHeight}px`;
  }
}

// --- Save, Try it now, and Cancel ---------------------------------------------
let saving = false;
editorForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (!draft || saving) return;
  stopCountdown();
  saving = true;
  saveButton.disabled = true;
  clearProblems();
  try {
    const answer = await invoke("actions_save", { was: draft.was, action: actionFrom(draft.fields) });
    actionsShown = ++actionsAsked;
    say(actionsNote, null);
    takeActions(answer);
    closeEditor();
  } catch (e) {
    showProblems(e);
    // A problem that is no field's, such as an action gone from the file,
    // means the list is out of date.
    if (!(e && Array.isArray(e.errors)) || e.errors.some((problem) => problem.field === null)) {
      loadActions();
    }
  } finally {
    saving = false;
    saveButton.disabled = false;
  }
});

// Try it now runs the action once, before it is saved. A keystroke goes to
// the app in front, so the page counts down three seconds first, and says
// why: there is time to switch to that app (D8). Pressed again, the count
// stops.
let countdown = null;
let trying = false;

tryButton.onclick = () => {
  if (!draft) return;
  if (countdown) {
    stopCountdown();
    return;
  }
  if (trying) return;
  clearProblems();
  showTry(null);
  const action = actionFrom(draft.fields);
  if (draft.fields.tab !== "keystroke") {
    runTry(action);
    return;
  }
  if (!action.keystroke.key_code && !action.keystroke.consumer_key_code) {
    showProblem("keys", "Record the keys, or pick them from the list, first.");
    return;
  }
  const ends = Date.now() + 3000;
  countdown = { timer: 0 };
  const tick = () => {
    const left = Math.ceil((ends - Date.now()) / 1000);
    if (left <= 0) {
      countdown = null;
      runTry(action);
      return;
    }
    tryNote.textContent = countdownWords(left);
    tryNote.hidden = false;
    countdown.timer = setTimeout(tick, 200);
  };
  tick();
  drawTryButton();
};

function stopCountdown() {
  if (!countdown) return;
  clearTimeout(countdown.timer);
  countdown = null;
  tryNote.hidden = true;
  drawTryButton();
}

async function runTry(action) {
  trying = true;
  tryNote.hidden = true;
  showTry("running");
  drawTryButton();
  try {
    showTry(await invoke("action_try", { action }));
  } catch (e) {
    showTry(null);
    showProblems(e);
  } finally {
    trying = false;
    drawTryButton();
  }
}

// How the try went, in the event log's words: exit 0, 204, done, or why not.
function showTry(outcome) {
  tryResult.hidden = outcome === null;
  if (outcome === null) {
    tryResult.replaceChildren();
    return;
  }
  if (outcome === "running") {
    tryResult.dataset.tone = "idle";
    tryResult.title = "Running";
    tryResult.replaceChildren(lucide("loader", "spin"), piece("", "running"));
    return;
  }
  const chip = resultChip(outcome);
  tryResult.dataset.tone = chip.tone;
  tryResult.title = chip.title;
  tryResult.replaceChildren(lucide(chip.mark), piece("", chip.words));
}

function drawTryButton() {
  tryButton.setAttribute("aria-pressed", String(countdown !== null));
  tryButton.querySelector("span").textContent = countdown ? "Stop" : "Try it now";
  tryButton.disabled = trying;
}

// A folded section hides the editor: what waits in it stops.
document.getElementById("actions").addEventListener("toggle", (event) => {
  if (event.target.open) return;
  stopRecording();
  stopLearning();
  stopCountdown();
});

// A learn or a recording this page left waiting before a reload would take
// the next press or key. Nothing here waits for them now.
invoke("learn_stop").catch(() => {});
invoke("record_keys_stop", {}).catch(() => {});
loadActions();
loadKeyNames();

// --- the input permission -------------------------------------------------
const banner = document.getElementById("permission");
// Until the actions are read, assume a keystroke: a missing warning is worse
// than one that goes away a moment later.
let postsKeys = true;
// Whether macOS lets this program post keys, or null until it answers. The
// banner asks for it when an action posts keys. The editor's Keystroke tab
// says so, and Record keys words its note by it.
let inputAllowed = null;

async function checkPermission() {
  try { inputAllowed = await invoke("input_permission"); } catch (e) { return; /* not up yet */ }
  banner.hidden = !postsKeys || inputAllowed;
  drawRecordNote();
}
// When macOS does not grant it at once, the banner says where to allow it.
const bannerWords = banner.querySelector("span");
document.getElementById("grant").onclick = async () => {
  try {
    const ok = await invoke("request_input_permission");
    banner.hidden = ok;
    inputAllowed = ok;
    drawRecordNote();
    if (!ok) {
      bannerWords.textContent = "Allow Mi Band Bridge in System Settings, Privacy & Security, " +
        "Device Control and Data Access, then reopen this app.";
    }
  } catch (e) { bannerWords.textContent = sentence(String(e)); }
};
checkPermission();
setInterval(checkPermission, 3000);

// --- the setup screen -----------------------------------------------------
// With no band paired, this is the whole window: the auth key, a name, and
// Pair device. The key field hides what is typed. The page reads it only as
// the pair starts, and empties it in the same step, so no page state holds
// the key, and nothing ever sends it back.
const setupView = document.getElementById("setup");
const setupBody = setupView.querySelector(".setup-body");
const pairForm = document.getElementById("pair-form");
const keyInput = document.getElementById("pair-key");
const nameInput = document.getElementById("pair-name");
const keyOk = document.getElementById("pair-key-ok");
const pairButton = document.getElementById("pair-submit");
const progressList = document.getElementById("pair-progress");
const pairError = document.getElementById("pair-error");

// A key is 32 hex digits, with or without 0x in front, as the account login
// prints it. The check mark says only that the shape is right: the band
// alone says whether the key is its own.
function keyLooksRight() {
  const text = keyInput.value.replace(/\s+/g, "").replace(/^0x/i, "");
  return /^[0-9a-f]{32}$/i.test(text);
}

function drawSetup() {
  // An svg has no hidden property, only the attribute.
  keyOk.toggleAttribute("hidden", !keyLooksRight());
  const filled = keyInput.value.trim() !== "" && nameInput.value.trim() !== "";
  pairButton.disabled = pairing || !filled;
  pairButton.setAttribute("aria-busy", String(pairing));
  pairButton.querySelector(".spinner").hidden = !pairing;
  pairButton.querySelector(".label").textContent = pairing ? "Pairing\u2026" : "Pair device";
  keyInput.disabled = pairing;
  nameInput.disabled = pairing;
}
keyInput.addEventListener("input", drawSetup);
nameInput.addEventListener("input", drawSetup);

// The band task writes for a terminal, in lower case. The setup screen
// shows each line as a sentence.
function sentence(text) {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

// One line of what the pair says as it goes. The newest carries the spinner
// while the pair runs. A wrong key keeps the search going for 25 seconds,
// and these lines are how a person sees it is still working.
function progress(text) {
  for (const line of progressList.children) line.classList.remove("now");
  const line = document.createElement("li");
  line.textContent = sentence(text);
  line.classList.toggle("now", pairing);
  progressList.append(line);
  fitHeight();
}

// A named error, or none, in an alert box.
function showAlert(box, text) {
  box.hidden = !text;
  box.querySelector("span").textContent = text ? sentence(text) : "";
  fitHeight();
}

document.getElementById("key-help").onclick = () =>
  invoke("open_key_help").catch((e) => showAlert(pairError, String(e)));

pairForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (pairing || pairButton.disabled) return;
  // Actions and settings that a forget kept go to the next band, or go.
  // The person says which before the pair starts.
  let saved;
  try {
    saved = JSON.parse(await invoke("saved_setup"));
  } catch (e) {
    showAlert(pairError, String(e));
    return;
  }
  if (keptWords(saved)) askReuse(saved);
  else pairWith("keep");
});

// Pair with the key in the field and the name beside it, and keep or delete
// what a forget kept.
async function pairWith(setup) {
  const name = nameInput.value.trim();
  pairing = true;
  progressList.replaceChildren();
  showAlert(pairError, null);
  // The key goes from the field into the request, and the field is emptied
  // in the same step.
  const answered = invoke("pair", { key: takeKey(), name, setup });
  drawSetup();
  drawPill();
  let failure = null;
  try {
    await answered;
  } catch (e) {
    failure = String(e);
  }
  // The pair's last lines may still wait to be collected.
  await drain();
  pairing = false;
  for (const line of progressList.children) line.classList.remove("now");
  drawSetup();
  drawPill();
  if (failure) {
    showAlert(pairError, failure);
    keyInput.focus();
    return;
  }
  nameInput.value = "";
  progressList.replaceChildren();
  drawSetup();
  loadPaired();
  showView("main");
  loadSettings();
  loadActions();
}

function takeKey() {
  const key = keyInput.value;
  keyInput.value = "";
  return key;
}

// --- forget, and the reuse on the next pair --------------------------------
// Forgetting a band and keeping what was built for it are two decisions, so
// the window asks them apart. Each sheet asks saved_setup fresh when it
// opens, rather than trust a copy from earlier.
const forgetButton = document.getElementById("forget");
const forgetSheet = document.getElementById("forget-sheet");
const forgetForm = document.getElementById("forget-form");
const forgetOptions = document.getElementById("forget-options");
const forgetError = document.getElementById("forget-error");
const forgetConfirm = document.getElementById("forget-confirm");
const forgetCancel = document.getElementById("forget-cancel");
const reuseSheet = document.getElementById("reuse-sheet");
const reuseForm = document.getElementById("reuse-form");
const reuseWarning = document.getElementById("reuse-warning");
const reuseConfirm = document.getElementById("reuse-confirm");
const forgetCopyKey = document.getElementById("forget-copy-key");
let forgetting = false;

// A forget takes the key out of the store, and after it the key is only
// where the account login gives it. So the sheet offers a copy first, and
// shows in itself why a copy failed.
const resetForgetCopy = copyKeyButton(forgetCopyKey, (text) => showAlert(forgetError, text));

// What a forget keeps, in words: "4 actions and your preferences",
// "1 action", "your preferences", or "" when nothing is set up. `saved` is
// the answer from saved_setup.
function keptWords(saved, adjective = "") {
  const count = saved.actions;
  const actions = count > 0 ? `${count} ${adjective}${count === 1 ? "action" : "actions"}` : "";
  const settings = saved.settingsChanged ? "your preferences" : "";
  return [actions, settings].filter(Boolean).join(" and ");
}

// "your 4 actions": a count takes "your" in front of it.
function yours(words) {
  return /^\d/.test(words) ? `your ${words}` : words;
}

// Whether the words take a plural verb. One action alone does not.
function several(saved) {
  return !(saved.actions === 1 && !saved.settingsChanged);
}

forgetButton.onclick = async () => {
  let saved;
  try {
    saved = JSON.parse(await invoke("saved_setup"));
  } catch (e) {
    setNote(sentence(String(e)), true);
    return;
  }
  paired = saved.device;
  if (!saved.device) {
    showView("setup");
    return;
  }
  const words = keptWords(saved);
  const plural = several(saved);
  document.getElementById("forget-title").textContent = `Forget \u201c${saved.device.name}\u201d?`;
  document.getElementById("forget-body").textContent = words
    ? "The device will be unpaired. Choose what happens to what you set up for it."
    : "The device will be unpaired.";
  forgetOptions.hidden = !words;
  document.getElementById("forget-keep").textContent =
    `${sentence(yours(words))} ${plural ? "stay" : "stays"} in config.toml. ` +
    `Pair another device later and you can reuse ${plural ? "them" : "it"}.`;
  document.getElementById("forget-delete").textContent =
    `${sentence(yours(words))} ${plural ? "are" : "is"} removed. This cannot be undone.`;
  forgetForm.elements.setup.value = "keep";
  showAlert(forgetError, null);
  resetForgetCopy();
  forgetSheet.showModal();
};

function drawForgetSheet() {
  forgetConfirm.disabled = forgetting;
  forgetCancel.disabled = forgetting;
  // The key is leaving the store: a copy now could find it there or not.
  forgetCopyKey.disabled = forgetting;
  forgetConfirm.textContent = forgetting ? "Forgetting\u2026" : "Forget device";
}

forgetForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (forgetting) return;
  const setup = forgetForm.elements.setup.value;
  forgetting = true;
  drawForgetSheet();
  try {
    const lines = await invoke("forget", { setup });
    forgetSheet.close();
    paired = null;
    band = null;
    progressList.replaceChildren();
    showAlert(pairError, null);
    showView("setup");
    lines.forEach((line) => progress(line));
    if (LET_GO_NOTE) progress(LET_GO_NOTE);
    loadSettings();
    loadActions();
  } catch (e) {
    showAlert(forgetError, String(e));
  } finally {
    forgetting = false;
    drawForgetSheet();
  }
});
forgetCancel.onclick = () => forgetSheet.close();
// The key may be leaving the store: Escape waits for it, as Cancel does.
forgetSheet.addEventListener("cancel", (event) => {
  if (forgetting) event.preventDefault();
});

function askReuse(saved) {
  const words = keptWords(saved);
  document.getElementById("reuse-body").textContent =
    `${sentence(words)} ${several(saved) ? "are" : "is"} still saved from your last device.`;
  reuseWarning.querySelector("span").textContent =
    `${sentence(yours(keptWords(saved, "saved ")))} will be deleted from config.toml. ` +
    "This cannot be undone.";
  reuseForm.elements.setup.value = "keep";
  drawReuse();
  reuseSheet.showModal();
}

// Start fresh deletes, so it warns, and its button says what it does.
function drawReuse() {
  const fresh = reuseForm.elements.setup.value === "delete";
  reuseWarning.hidden = !fresh;
  reuseConfirm.textContent = fresh ? "Delete and start fresh" : "Pair device";
  reuseConfirm.className = fresh ? "destructive" : "accent";
}
reuseForm.addEventListener("change", drawReuse);
reuseForm.addEventListener("submit", (event) => {
  event.preventDefault();
  const setup = reuseForm.elements.setup.value;
  reuseSheet.close();
  pairWith(setup);
});
document.getElementById("reuse-cancel").onclick = () => reuseSheet.close();

// --- the two views ----------------------------------------------------------
function showView(next) {
  view = next;
  document.body.dataset.view = next;
  if (next === "setup") closeEditor();
  drawPill();
  drawConnection();
  drawLive();
  if (next === "setup") {
    drawSetup();
    keyInput.focus();
  } else {
    refreshDevice();
  }
  fitHeight();
}

// Which view to open on: saved_setup says whether a band is paired. The band
// task starts as the window does, so ask again for a moment if it is not up.
// A file that does not load fails every try alike, so it is said at once.
function chooseView(tries) {
  invoke("saved_setup")
    .then((text) => {
      paired = JSON.parse(text).device;
      showView(paired ? "main" : "setup");
    })
    .catch((e) => {
      const text = String(e);
      const broken = isConfigError(text);
      if (tries > 0 && !broken) return setTimeout(() => chooseView(tries - 1), 300);
      // With nothing to go on, show the sections, and say why: a file that
      // does not load in the note under the title bar, anything else under
      // the card.
      showView("main");
      if (broken) loadActions();
      else setNote(sentence(text), true);
    });
}

// Say plainly when the bridge is missing, rather than looking merely idle.
if (!bridge.core || !bridge.core.invoke) {
  unreachable = true;
  showView("main");
  setNote("This window cannot reach the app. Nothing here will work.", true);
  console.error("The bridge offers only: " + Object.keys(bridge).join(", "));
} else {
  chooseView(10);
}

// --- the caption buttons (Windows) ------------------------------------------
// On Windows the window has no frame (W8), so the page draws Windows' buttons
// at the top right: minimize, maximize or restore, and close. Close puts the
// window away, as the Mac's does: BandBridge keeps running in the tray, and
// its menu has Quit. Maximize zooms, as the Mac's zoom does: the full height
// of the screen at the window's width, since the page is drawn 700 points
// wide. A double click on the bar zooms too. The middle button shows what it
// does now, so the page asks each time the window's size changes. Windows
// dims the buttons of a window that is not in front, and so does the page.
if (onWindows) {
  const frame = bridge.window && bridge.window.getCurrentWindow
    ? bridge.window.getCurrentWindow()
    : null;
  const ask = (call) => {
    if (frame) frame[call]().catch((e) => console.error(`the window did not ${call}`, e));
  };
  const maximize = document.getElementById("caption-maximize");
  const drawMaximize = (zoomed) => {
    const words = zoomed ? "Restore down" : "Maximize";
    maximize.textContent = zoomed ? "" : "";
    maximize.title = words;
    maximize.setAttribute("aria-label", words);
  };
  const zoom = () => invoke("zoom").then(drawMaximize).catch((e) => console.error("no zoom", e));
  document.getElementById("caption-minimize").onclick = () => ask("minimize");
  maximize.onclick = zoom;
  document.getElementById("caption-close").onclick = () => ask("close");
  // Windows' caption buttons never take the keyboard's focus, so a field
  // being typed in keeps it.
  for (const button of document.querySelectorAll("button.caption")) {
    button.addEventListener("mousedown", (event) => event.preventDefault());
  }
  // The second press of a double click, as the bar's own drag code reads
  // one. A double click on a button is the button's.
  document.querySelector(".titlebar").addEventListener("mousedown", (event) => {
    if (event.button === 0 && event.detail === 2 && !event.target.closest("button")) zoom();
  });
  const drawInFront = () => {
    document.documentElement.classList.toggle("window-inactive", !document.hasFocus());
  };
  const askZoomed = () => invoke("zoomed").then(drawMaximize).catch(() => {});
  window.addEventListener("resize", askZoomed);
  window.addEventListener("focus", drawInFront);
  window.addEventListener("blur", drawInFront);
  askZoomed();
  drawInFront();
}

// --- the status bar -------------------------------------------------------
const statusBar = document.querySelector(".statusbar");
const configPath = document.getElementById("config-path");
const statusNote = document.getElementById("status-note");
invoke("config_path").then((shown) => {
  configPath.textContent = shown;
  configPath.title = shown;
}).catch((e) => {
  configPath.textContent = sentence(String(e));
  configPath.classList.add("bad");
});

// Why Reveal in Finder did not, in the bar, in place of the path for a
// moment.
let statusTimer = 0;
function statusSays(text) {
  clearTimeout(statusTimer);
  statusBar.classList.toggle("failed", Boolean(text));
  statusNote.textContent = text ? sentence(text) : "";
  statusNote.title = statusNote.textContent;
  statusNote.hidden = !text;
  if (text) statusTimer = setTimeout(() => statusSays(null), 8000);
}
document.getElementById("reveal").onclick = () => {
  statusSays(null);
  invoke("reveal_config").catch((e) => statusSays(String(e)));
};

// About: this build's version, and the licenses and notices it opens in the
// browser, each by its name.
invoke("version")
  .then((version) => { document.getElementById("about-version").textContent = version; })
  .catch(() => {});
for (const link of document.querySelectorAll("#about [data-page]")) {
  link.onclick = () => {
    statusSays(null);
    invoke("open_about_page", { name: link.dataset.page }).catch((e) => statusSays(String(e)));
  };
}

// Ask the window to be as tall as what it shows, so it opens with no
// scrollbar, and no taller than the screen. Measured rather than guessed: a
// guess is wrong on any change to the sections, the font, or the platform.
// The view scrolls between the bars, so the page itself never grows: what it
// needs is the bars plus everything the view holds. For the sections, that
// is measured from the first section's top to the last one's bottom, because
// scrollHeight never reads less than the room the sections already have, and
// the window could then grow but never shrink. The setup screen is one block,
// measured whole.
function fitHeight() {
  if (view === "loading") return;
  const shown = view === "setup" ? setupView : sections;
  let content;
  if (view === "setup") {
    content = setupBody.getBoundingClientRect().height;
  } else {
    const first = sections.firstElementChild.getBoundingClientRect();
    const last = sections.lastElementChild.getBoundingClientRect();
    content = last.bottom - first.top;
  }
  const needed = window.innerHeight - shown.clientHeight + Math.ceil(content);
  const wanted = Math.min(needed, window.screen.availHeight);
  const have = window.innerHeight;
  if (Math.abs(wanted - have) >= 2) {
    invoke("adjust_height", { delta: wanted - have }).catch(() => {});
  }
}
window.addEventListener("load", () => setTimeout(fitHeight, 60));

// Where the connection stands when the window opens. The band task starts
// after the page loads, so keep asking until it answers. A state line that
// came first is as new or newer, and wins.
(function askWhereItStands(tries) {
  if (unreachable) return;
  invoke("connection")
    .then((text) => {
      if (!standing) setStanding(JSON.parse(text));
    })
    .catch((e) => {
      // A collected line may already have said what went wrong. Saying it a
      // second time here would read as a second fault.
      if (standing) return;
      if (tries > 0) return setTimeout(() => askWhereItStands(tries - 1), 500);
      setNote(sentence(String(e)), true);
    });
})(40);
