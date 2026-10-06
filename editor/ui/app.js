// The editor window. It holds no logic of its own: every check, every file,
// and every build happens in Rust through the commands below.
const tauri = window.__TAURI__ || {};
const invoke = tauri.core && tauri.core.invoke
  ? tauri.core.invoke
  : () => Promise.reject(new Error("The editor window cannot reach the app."));

const $ = (id) => document.getElementById(id);
const { autoPressColor, readHex } = window.BandColors;
const state = {
  folder: null, apps: [], appDir: null, layout: null, errors: 0, art: {}, thumbs: {},
  // The build tools are missing: the notice shows, and nothing else works.
  // The commands that set them up, for Copy.
  toolsMissing: false, steps: [],
  // The button whose row is open, counted from 0, or null. One row opens at
  // a time.
  open: null,
  // The last check report and the next free signal, both from Rust.
  report: { errors: [], warnings: [], settings: [] },
  next: null,
  // When the app last built, until the next change makes that build old.
  built: null,
  // What BandBridge said last, from the title bar's poll: "connected",
  // "not_connected", "not_running", or null before it answers. Install on
  // band waits for "connected".
  bridge: null,
};
let timer = null;
let imageUrls = [];

// The band's screen, in band pixels, and how large the preview draws it.
const SCREEN_H = 490;
const SCALE = 1.3;
// The most the free strip can take: the screen, less its top margin.
const MAX_STRIP = 484;

function hexColor(n) {
  return "#" + Number(n).toString(16).padStart(6, "0");
}

// An element, with its properties and its children: el("span.tag", {}, "Picture").
function el(spec, props = {}, ...children) {
  const [tag, ...classes] = spec.split(".");
  const node = document.createElement(tag);
  if (classes.length) node.className = classes.join(" ");
  for (const [key, value] of Object.entries(props)) {
    if (key === "dataset") Object.assign(node.dataset, value);
    else if (key.includes("-") || key === "role") node.setAttribute(key, value);
    else node[key] = value;
  }
  node.append(...children.filter((child) => child !== null && child !== undefined && child !== false));
  return node;
}

// An icon from the page's sprite.
function icon(name) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("class", "icon");
  svg.setAttribute("aria-hidden", "true");
  const use = document.createElementNS("http://www.w3.org/2000/svg", "use");
  use.setAttribute("href", `#i-${name}`);
  svg.append(use);
  return svg;
}

// "1 error", "2 warnings".
function counted(n, noun) {
  return `${n} ${noun}${n === 1 ? "" : "s"}`;
}

// The last part of a path, whichever way the system writes its separators.
function baseName(path) {
  return String(path).split(/[\\/]/).filter(Boolean).pop() || "";
}

// What went wrong: in the Output drawer while an app is open, which opens
// to show it, and under the start card before one is. show("") clears the
// start card's line.
function show(text) {
  $("start-note").textContent = text;
  $("start-note").hidden = !text;
  if (text && !$("main").hidden) {
    addLine({ at: Date.now(), text, tone: "bad", percent: null });
    setDrawer(true);
  }
}

// The Output drawer (decisions E4 and E5). Build and install say each step
// as it happens, through the "output" event, and the drawer keeps them, line
// by line with their times, until Clear. It stays closed until the first
// build or install, and each one opens it.
const drawer = {
  open: false,
  // Whether a build or an install has run in this window.
  ran: false,
  // What runs: { kind: "build" or "install", percent, appDir }, or null.
  run: null,
  // The last run, when it stopped: { kind, how, appDir }, where how is
  // "stopped", or "not_running" and "not_connected", the two stops of an
  // install that its card shows. Null when it finished or was cleared.
  ended: null,
  // The line the upload's progress keeps to, while it has one.
  progress: null,
  // Whether the log keeps its newest line in view: until the person
  // scrolls back, and again once they scroll to the end.
  follow: true,
};

function setDrawer(open) {
  drawer.open = open;
  $("drawer").classList.toggle("open", open);
  $("drawer-toggle").setAttribute("aria-expanded", String(open));
  $("drawer-body").hidden = !open;
  followLog();
  showDrawerHead();
}

// The log's newest line in view, unless the person has scrolled back. A
// hidden log forgets where it was, and a narrower one wraps its lines
// longer, so this runs when the drawer opens and the window changes size.
function followLog() {
  if (drawer.follow) $("log").scrollTop = $("log").scrollHeight;
}

// "Installing… 62%" on the Install button and in the drawer's head, and
// "Installing…" until the upload says how far it is.
function installWords(run) {
  return run.percent === null ? "Installing…" : `Installing… ${run.percent}%`;
}

// The drawer's head: what runs, or what stopped, in its chip. With neither,
// its note says why there is nothing to read: an error stops the build, or
// nothing has run yet.
function showDrawerHead() {
  const { run, ended } = drawer;
  const chip = run
    ? { tone: "busy", icon: "loader", words: run.kind === "build" ? "Building…" : installWords(run) }
    : ended ? { tone: "bad", icon: "circle-x", words: ended.kind === "build" ? "Build stopped" : "Install stopped" }
    : null;
  $("run-chip").hidden = !chip;
  if (chip) {
    $("run-chip").dataset.tone = chip.tone;
    $("run-icon").setAttribute("href", `#i-${chip.icon}`);
    $("run-icon").parentNode.classList.toggle("spin", chip.tone === "busy");
    $("run-words").textContent = chip.words;
  }
  const n = state.errors;
  $("drawer-note").textContent = chip ? ""
    : n ? `Build stopped: ${counted(n, "error")}`
    : drawer.ran ? "" : "No build yet";
  $("drawer-note").hidden = !$("drawer-note").textContent;
  const something = $("log").childElementCount > 0 || !$("stop-card").hidden;
  $("drawer-clear").hidden = !drawer.open || !something;
  $("drawer-show").hidden = drawer.open;
}

// The card of an install BandBridge could not take, with Install again.
function showStopCard() {
  const { ended } = drawer;
  const how = ended && ended.kind === "install" ? ended.how : null;
  $("stop-card").hidden = how !== "not_running" && how !== "not_connected";
  $("stop-title").textContent = how === "not_running" ? "Mi Band Bridge is not running."
    : how === "not_connected" ? "The band is not connected." : "";
}

// 14:02:11, as the log writes a line's time.
function clockSeconds(date) {
  return [date.getHours(), date.getMinutes(), date.getSeconds()]
    .map((n) => String(n).padStart(2, "0")).join(":");
}

// One line a build or an install said. A line of the upload's progress
// takes the place of the last one, so the upload keeps to one line.
function addLine(said) {
  const log = $("log");
  const progress = said.percent !== null && said.percent !== undefined;
  let line = progress ? drawer.progress : null;
  if (!line) {
    line = el("div.log-line", {}, el("span.log-time"), el("span.log-text"));
    log.append(line);
    if (progress) drawer.progress = line;
  }
  line.dataset.tone = said.tone;
  line.firstChild.textContent = clockSeconds(new Date(said.at));
  line.lastChild.textContent = said.text;
  log.hidden = false;
  followLog();
  if (progress && drawer.run && drawer.run.kind === "install") {
    drawer.run.percent = said.percent;
    updateToolbar();
  }
  showDrawerHead();
}

// A build or an install starts: the drawer opens, and the toolbar says so.
function beginRun(kind, appDir) {
  drawer.run = { kind, percent: null, appDir };
  drawer.ended = null;
  drawer.progress = null;
  drawer.ran = true;
  showStopCard();
  setDrawer(true);
  showRun();
}

// It ended: "built", "installed", or how it stopped. An install leaves
// BandBridge somewhere new: stopped, or connecting the band again. So the
// page asks it at once, and the title bar and the install buttons follow.
function endRun(how) {
  const { run } = drawer;
  drawer.run = null;
  drawer.ended = how === "built" || how === "installed" ? null : { kind: run.kind, how, appDir: run.appDir };
  showStopCard();
  showRun();
  if (run.kind === "install") askBridge();
}

// What a run changes beyond the drawer: the toolbar, and, while it builds,
// the panes and the toolbar's other controls, which wait for it. A build
// writes the folder the preview draws from, and loads the saved app when it
// ends, so a change made meanwhile would be lost. An install takes a
// minute, and leaves them all to use.
function showRun() {
  const building = Boolean(drawer.run && drawer.run.kind === "build");
  for (const part of document.querySelectorAll(".app-pane, .buttons-part, .preview-pane, #folder, .picker, .new-app")) {
    part.inert = building;
  }
  showDrawerHead();
  updateToolbar();
}

// Run an event handler and route a rejection to show(), so no invoke
// failure stays silent in the devtools console.
function guarded(fn) {
  return (...args) => Promise.resolve().then(() => fn(...args)).catch((e) => show(String(e)));
}

// Save, check, and redraw a moment after the last change. A change to the
// app makes the last build old. A dismissed warning does not reach the
// generator, so it keeps the build: `schedule({ keepBuild: true })`.
function schedule({ keepBuild = false } = {}) {
  if (!keepBuild) state.built = null;
  clearTimeout(timer);
  timer = setTimeout(() => {
    timer = null;
    refresh().catch((e) => show(String(e)));
  }, 250);
}

// Keep the last edit of the app we are leaving. Only flush when a save is
// actually pending, so switching apps does not save and check for nothing.
async function flush() {
  if (timer === null) return;
  clearTimeout(timer);
  timer = null;
  if (state.appDir && state.layout) await refresh();
}

// The body shows the start card until an app is open, then the three
// panes. With the build tools missing, it shows their notice instead.
function showView(view) {
  $("start").hidden = view !== "start";
  $("tools").hidden = view !== "tools";
  $("main").hidden = view !== "app";
}

// Which controls of the toolbar work, and Install again on the stop card,
// by the rules in toolbar.js. The note before Build says when an error stops
// it, and the title bar says when BandBridge's band is not connected. While
// an install runs, Install on band shows how far it is.
function updateToolbar() {
  const open = Boolean(state.appDir && state.layout);
  const blocked = open && state.errors > 0;
  const { run } = drawer;
  const installing = Boolean(run && run.kind === "install");
  const on = window.BandToolbar.controls({
    toolsMissing: state.toolsMissing, folder: Boolean(state.folder), apps: state.apps.length,
    open, errors: state.errors, run: run ? run.kind : null, bridge: state.bridge,
  });
  $("folder").disabled = !on.folder;
  $("apps").disabled = !on.apps;
  $("new-name").disabled = !on.newApp;
  $("new-app").disabled = !on.newApp;
  $("build").disabled = !on.build;
  $("install").disabled = !on.install;
  $("install-again").disabled = !on.again;
  $("install").classList.toggle("busy", installing);
  $("install-words").textContent = installing ? installWords(run) : "Install on band";
  $("install-icon").setAttribute("href", installing ? "#i-loader" : "#i-download");
  $("install-icon").parentNode.classList.toggle("spin", installing);
  $("blocked").hidden = !blocked;
  $("blocked").textContent = blocked
    ? `${counted(state.errors, "error")} ${state.errors === 1 ? "stops" : "stop"} the build`
    : "";
}

async function start() {
  askBridge();
  const tools = await invoke("tools_status");
  if (!tools.ready) {
    showTools(tools);
    return;
  }
  let saved = null;
  try {
    saved = localStorage.getItem("folder");
  } catch (e) {
    saved = null;
  }
  if (saved) await openFolder(saved);
  else showView("start");
}

// Open a folder of apps, and in it the app `dir`, or the first one.
async function openFolder(folder, dir) {
  await flush();
  state.folder = folder;
  try {
    localStorage.setItem("folder", folder);
  } catch (e) {
    // The folder is only a convenience to remember.
  }
  const path = $("folder-path");
  path.textContent = await invoke("shown_folder", { folder });
  path.title = folder;
  path.classList.remove("none");
  const apps = await invoke("list_apps", { folder });
  showApps(apps);
  if (apps.length) {
    // A broken layout.json in the first app must not stop the folder from
    // opening, so report it here instead of letting it stop the caller.
    try {
      await openApp(dir || apps[0].dir);
    } catch (e) {
      show(String(e));
    }
  } else {
    closeApp();
    // A first app starts with its name.
    $("new-name").focus();
  }
}

// The app picker: the folder's apps by name, and how many there are.
function showApps(apps) {
  state.apps = apps;
  $("apps").replaceChildren(...apps.map((a) => new Option(a.name, a.dir)));
  $("app-count").textContent = apps.length;
  showAppName();
  updateToolbar();
}

// The picker's words: the chosen app's name, or "No apps". A long name is
// cut short, and its tooltip holds all of it.
function showAppName() {
  const chosen = $("apps").selectedOptions[0];
  $("app-name").textContent = chosen ? chosen.text : "No apps";
  $("app-name").classList.toggle("none", !chosen);
  $("app-picker").title = chosen ? chosen.text : "";
}

async function openApp(dir) {
  await flush();
  // A stopped install's card, and its Install again, belong to its own app.
  if (dir !== state.appDir && !drawer.run) {
    drawer.ended = null;
    showStopCard();
  }
  let layout;
  try {
    layout = await invoke("load_layout", { appDir: dir });
  } catch (e) {
    // The old app's state must not leak into the new app's file.
    closeApp();
    throw e;
  }
  forgetPictures();
  state.appDir = dir;
  state.layout = layout;
  state.open = null;
  state.built = null;
  state.errors = 0;
  state.report = { errors: [], warnings: [], settings: [] };
  $("apps").value = dir;
  showAppName();
  showView("app");
  show("");
  fillForm();
  await refresh();
  updateToolbar();
}

// No app open: the start card, and nothing to build or install. What was said
// before, such as the last app's build log, is not news for the card, so the
// card starts clean. A caller that closes the app because it failed to load
// says why after this. The picker names the app that was picked, even one
// that did not open.
function closeApp() {
  show("");
  forgetPictures();
  state.appDir = null;
  state.layout = null;
  state.errors = 0;
  state.open = null;
  showView("start");
  showAppName();
  updateToolbar();
}

function fillForm() {
  const l = state.layout;
  $("name").value = l.name;
  $("app-file").textContent = `${baseName(state.appDir)}/layout.json`;
  $("app-file").title = state.appDir;
  $("color-well").replaceChildren(colorWell(l.color, "Color", (color) => {
    l.color = color;
    schedule();
  }));
  // Nothing else shows the app icon, so the form shows the made file itself.
  $("icon-thumb").title = l.icon || "A plain disc in the app color";
  $("icon-clear").hidden = !l.icon;
  choose($("columns"), l.columns);
  choose($("corners"), l.corners);
  showStripValue(l.bottomStrip);
  renderButtons();
  paintThumbs();
}

// The app icon, as the generator made it. Nothing else on screen shows it,
// and what the band draws is a disc when the user gave no picture of their own.
function paintThumbs() {
  for (const thumb of document.querySelectorAll("img[data-art]")) {
    const url = state.art[thumb.dataset.art];
    if (url) thumb.src = url;
    else thumb.removeAttribute("src");
  }
}

// A button's own picture, scaled by Rust. The row shows the picture alone,
// because the preview beside it already shows the whole button.
async function pictureUrl(rel) {
  if (!state.thumbs[rel]) {
    const bytes = await invoke("image_thumbnail", { appDir: state.appDir, rel });
    state.thumbs[rel] = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: "image/png" }));
  }
  return state.thumbs[rel];
}

// A picture that cannot be read leaves its box empty. The checks name it.
function picture(spec, rel) {
  const img = el(spec, { alt: "" });
  pictureUrl(rel).then((url) => { img.src = url; }, () => {});
  return img;
}

// Every picture of the app we are leaving. A file only changes when the user
// picks another one, which gets another name, so holding these is safe.
function forgetPictures() {
  Object.values(state.thumbs).forEach((url) => URL.revokeObjectURL(url));
  state.thumbs = {};
}

// Press the segment of a choice that holds `value`.
function choose(group, value) {
  for (const segment of group.querySelectorAll("button")) {
    segment.setAttribute("aria-pressed", String(segment.dataset.value === String(value)));
  }
}

// A color well. The swatch opens the system's color picker, and the hex
// beside it takes a typed #rrggbb, which counts once it is a whole color.
// A color the layout holds that is not one, from a hand edit, shows as it
// is, and the checks say what is wrong with it.
function colorWell(initial, label, onchange) {
  let color = initial;
  const picker = el("input", { type: "color", "aria-label": label });
  const swatch = el("span.swatch", {}, picker);
  const hex = el("input.hex", { value: color, spellcheck: false, "aria-label": `${label} as #rrggbb` });
  const paint = (value) => {
    const valid = readHex(value);
    swatch.style.background = valid || "transparent";
    if (valid) picker.value = valid;
  };
  const set = (value) => {
    color = value;
    paint(value);
    onchange(value);
  };
  paint(color);
  picker.oninput = () => {
    hex.value = picker.value;
    set(picker.value);
  };
  hex.oninput = () => {
    const valid = readHex(hex.value);
    if (valid) set(valid);
  };
  // Left half typed, the field shows the color the layout holds again.
  hex.onchange = () => { hex.value = readHex(hex.value) || color; };
  return el("span.well", {}, swatch, hex);
}

// The free strip, in its number field and on its slider.
function showStripValue(px) {
  $("strip").value = px;
  $("strip-slider").value = Math.min(px, MAX_STRIP);
  $("strip-slider").style.setProperty("--at", String(Math.min(px, MAX_STRIP) / MAX_STRIP));
}

function setStrip(px) {
  state.layout.bottomStrip = px;
  showStripValue(px);
  schedule();
}

// The rows of the Buttons pane, one per button. The open row holds the
// button's editor.
function renderButtons() {
  const l = state.layout;
  if (state.open !== null && state.open >= l.buttons.length) state.open = null;
  const list = $("buttons");
  // Every index takes the width of the longest, so the swatches line up.
  list.style.setProperty("--digits", String(String(l.buttons.length).length));
  list.replaceChildren(...l.buttons.map(buttonRow));
  list.hidden = l.buttons.length === 0;
  $("button-count").textContent = l.buttons.length;
  decorate();
}

function rowOf(i) {
  return $("buttons").querySelector(`[data-row="${i}"]`);
}

// Open a button's row, or close it when it is the open one. The rows are
// drawn again, so a keyboard user keeps their place on the row's disclosure.
function toggle(i, keepFocus) {
  state.open = state.open === i ? null : i;
  renderButtons();
  if (keepFocus) focusRow(i);
}

function focusRow(i) {
  const row = rowOf(i);
  if (row) row.querySelector(".disclosure").focus({ preventScroll: true });
}

// "Show button": open the row, and bring it into view. From the keyboard,
// the focus goes with it.
function showButton(i, keepFocus) {
  if (!state.layout || i === null || i === undefined || i >= state.layout.buttons.length) return;
  state.open = i;
  renderButtons();
  rowOf(i).scrollIntoView({ block: "nearest" });
  if (keepFocus) focusRow(i);
}

function buttonRow(b, i) {
  const l = state.layout;
  const open = state.open === i;
  const label = el("span.brow-label", { title: b.label || "" }, b.label || "");
  if (b.image) label.classList.add("muted");
  const tags = [];
  if (b.image) tags.push(el("span.tag", {}, icon("image"), "Picture"));
  // With one column every button is full width, so the tag only means
  // something with two.
  if (b.span && l.columns > 1) tags.push(el("span.tag", {}, icon("move-horizontal"), "Full width"));
  const swatch = b.image
    ? picture("img.chip22", b.image)
    : el("span.chip22", { style: `background: ${readHex(b.color) || "transparent"}` });
  const pill = el("span.sig", { title: b.signal }, b.signal);

  const move = (by) => {
    const j = i + by;
    if (j < 0 || j >= l.buttons.length) return;
    [l.buttons[i], l.buttons[j]] = [l.buttons[j], l.buttons[i]];
    // The open row goes where its button goes.
    if (state.open === i) state.open = j;
    else if (state.open === j) state.open = i;
    renderButtons();
    schedule();
  };
  const remove = () => {
    l.buttons.splice(i, 1);
    if (state.open === i) state.open = null;
    else if (state.open !== null && state.open > i) state.open -= 1;
    renderButtons();
    schedule();
  };
  const name = b.label ? `button ${i + 1}, ${b.label}` : `button ${i + 1}`;
  const named = (words) => ({ title: words, "aria-label": words });
  const actions = el("span.row-actions", {},
    el("button.icon-btn", { ...named(`Move button ${i + 1} up`), disabled: i === 0, onclick: () => move(-1) }, icon("arrow-up")),
    el("button.icon-btn", { ...named(`Move button ${i + 1} down`), disabled: i === l.buttons.length - 1, onclick: () => move(1) }, icon("arrow-down")),
    el("button.icon-btn.remove", { ...named(`Remove button ${i + 1}`), onclick: remove }, icon("trash-2")),
  );
  const disclosure = el("button.disclosure", {
    "aria-expanded": String(open), "aria-label": name, onclick: (event) => { event.stopPropagation(); toggle(i, true); },
  }, icon(open ? "chevron-up" : "chevron-down"));
  const head = el("div.brow-head", {},
    el("span.index", {}, String(i + 1)), swatch, el("span.brow-text", {}, label, ...tags), pill, actions, disclosure);
  // A click anywhere on the row, but on its own buttons, opens or closes it.
  head.onclick = (event) => {
    if (!event.target.closest("button")) toggle(i);
  };
  const row = el("div.brow", { dataset: { row: String(i) } }, head);
  if (open) {
    row.classList.add("open");
    row.append(buttonEditor(b, i, { label, pill, swatch }));
  }
  return row;
}

// The open row's editor: every setting of one button.
function buttonEditor(b, i, head) {
  const l = state.layout;
  const labelInput = el("input.label-input", { value: b.label || "", spellcheck: false, "aria-label": "Label" });
  const labelField = el("span.field", {}, icon("tag"), labelInput);
  // The band draws a label across the middle of a button and cannot move it,
  // so a picture and a label cannot share one. The picture wins, and the
  // label waits in the file until the picture goes. The note says so while
  // there is a label to wait.
  const note = b.image ? el("span.enote", {}, icon("eye-off"), "Not shown while there is a picture") : null;
  if (b.image) labelField.classList.add("muted");
  if (note) note.hidden = !b.label;
  labelInput.oninput = () => {
    b.label = labelInput.value;
    head.label.textContent = b.label;
    head.label.title = b.label;
    if (note) note.hidden = !b.label;
    schedule();
  };
  const signalInput = el("input.signal-input", { value: b.signal, spellcheck: false, "aria-label": "Signal" });
  signalInput.oninput = () => {
    b.signal = signalInput.value;
    head.pill.textContent = b.signal;
    head.pill.title = b.signal;
    schedule();
  };
  const labelSignal = el("div.erow", {},
    el("div.efield.grow", {},
      el("span.cap", {}, "Label"), labelField, note),
    el("div.efield.signal", {}, el("span.cap", {}, "Signal"), el("span.field.signal-box", {}, signalInput)),
  );

  const colors = el("div.erow", {},
    el("div.efield.color", {}, el("span.cap", {}, "Color"), colorWell(b.color, "Color", (color) => {
      b.color = color;
      if (!b.image) head.swatch.style.background = color;
      showPress();
      schedule();
    })),
    el("div.efield.grow", {}, el("span.cap", {}, "Pressed color"), pressedColor(b)),
  );
  // An automatic pressed color follows the button's color, so its swatch is
  // drawn again whenever the color changes. It shows the shade the band will
  // draw, and choosing a color of one's own starts from it.
  const showPress = () => {
    const auto = colors.querySelector(".auto .swatch");
    if (auto) auto.style.background = readHex(autoPressColor(b.color)) || "transparent";
  };

  const pick = el("button.btn", { onclick: guarded(async () => {
    const rel = await invoke("pick_image", { appDir: state.appDir });
    if (rel) { b.image = rel; renderButtons(); schedule(); }
  }) }, "Choose image…");
  const pictureRow = b.image
    ? el("div.picrow", {},
        picture("img.pic.picture-box", b.image),
        el("div.pic-text", {}, el("span.pic-file", { title: b.image }, baseName(b.image)), el("span.help", {}, "Fills the whole button")),
        pick,
        el("button.btn.dim", { onclick: () => { b.image = null; renderButtons(); schedule(); } }, "Remove"))
    : el("div.picrow", {},
        el("span.pic.empty.picture-box", {}, icon("image")),
        el("p.pic-help", {}, "PNG or JPEG. A picture fills the button and replaces the label."),
        pick);

  // Full width only means something with two columns. With one, the switch
  // stays in its place, and its help says why it does nothing.
  const span = el("button.switch", {
    role: "switch", "aria-checked": String(Boolean(b.span)), "aria-label": "Full width",
    disabled: l.columns < 2,
    onclick: () => { b.span = !b.span; renderButtons(); schedule(); },
  });
  const fullWidth = el("div.fullwidth", {}, span,
    el("div", {}, el("span.setting-label", {}, "Full width"), el("span.help", {}, "Spans both columns. Only with 2 columns.")));

  return el("div.editor", {},
    labelSignal,
    el("div.row-errors"),
    colors,
    el("div.efield", {}, el("span.cap", {}, "Picture"), pictureRow),
    fullWidth,
  );
}

// Pressed color: Automatic follows the color, or Choose sets one.
function pressedColor(b) {
  const segments = el("span.seg", { role: "group", "aria-label": "Pressed color" },
    el("button", { type: "button", dataset: { value: "auto" } }, "Automatic"),
    el("button", { type: "button", dataset: { value: "choose" } }, "Choose"));
  const box = el("span.pressed", {}, segments);
  const show = () => {
    choose(segments, b.pressColor ? "choose" : "auto");
    const detail = b.pressColor
      ? colorWell(b.pressColor, "Pressed color", (color) => { b.pressColor = color; schedule(); })
      : el("span.auto", {}, el("span.swatch", { style: `background: ${readHex(autoPressColor(b.color)) || "transparent"}` }), "follows the color");
    detail.classList.add("press-box");
    box.replaceChildren(segments, detail);
  };
  segments.onclick = (event) => {
    const segment = event.target.closest("button");
    if (!segment) return;
    const chosen = segment.dataset.value === "choose";
    if (chosen === Boolean(b.pressColor)) return;
    // A chosen color starts from the shade the band draws now.
    b.pressColor = chosen ? readHex(autoPressColor(b.color)) || "#ffffff" : null;
    show();
    schedule();
  };
  show();
  return box;
}

// What the last report says about the rows: a red signal where buttons
// share one or it cannot be sent, and in the open row, each field at fault
// and what to do.
function decorate() {
  if (!state.layout) return;
  const errors = state.report.errors;
  const badSignals = new Set();
  for (const f of errors) {
    if (f.field === "signal" && f.button !== null && f.button !== undefined) badSignals.add(f.button);
    for (const i of f.sameSignal || []) badSignals.add(i);
  }
  for (const row of $("buttons").children) {
    const i = Number(row.dataset.row);
    row.querySelector(".sig").classList.toggle("bad", badSignals.has(i));
    if (i !== state.open) continue;
    const said = rowErrors(errors, i);
    row.classList.toggle("bad", said.length > 0);
    for (const [field, selector] of [["signal", ".signal-box"], ["color", ".efield.color .well"], ["pressColor", ".press-box"], ["image", ".picture-box"]]) {
      const box = row.querySelector(selector);
      if (box) box.classList.toggle("bad", said.some((s) => s.field === field));
    }
    row.querySelector(".row-errors").replaceChildren(...said.map((s) => rowError(s, i)));
  }
}

// What a button's own row says: each error about it. Of buttons that send
// one signal, the first names the buttons that repeat it, and each repeat
// names the first.
function rowErrors(errors, i) {
  const said = [];
  for (const f of errors) {
    const group = f.sameSignal || [];
    if (group.includes(i)) said.push({ field: "signal", text: group[0] === i ? f.firstNote : f.note });
    else if (f.button === i) said.push({ field: f.field, text: f.note || f.message });
  }
  return said;
}

// One error in the open row, with the next free signal to use where the
// signal is what is wrong. It sets this row's own signal.
function rowError(said, i) {
  const fix = said.field === "signal" && state.next
    ? el("button.btn.small", { onclick: () => useSignal(i, state.next) }, `Use ${state.next}`)
    : null;
  return el("div.row-error", {}, icon("circle-x"), el("span", {}, said.text), fix);
}

function useSignal(i, signal) {
  state.layout.buttons[i].signal = signal;
  renderButtons();
  schedule();
}

async function refresh() {
  const l = state.layout;
  if (!l) return;
  await invoke("save_layout", { appDir: state.appDir, layout: l });
  const report = await invoke("check_layout", { appDir: state.appDir, layout: l });
  state.report = report;
  state.errors = report.errors.length;
  showReport(report);
  updateToolbar();
  showNext(await invoke("next_signal", { appDir: state.appDir, layout: l }));
  let data;
  try {
    data = await invoke("preview", { appDir: state.appDir, layout: l });
  } catch (e) {
    // An error such as two buttons with one signal still leaves a page to
    // draw. One the page cannot be made with, such as a color that is not
    // one, leaves nothing to draw.
    if (!report.errors.length) throw e;
    $("screen").replaceChildren();
    showStrip();
    showSent(undefined);
    showProblems([report.errors.length === 1 ? "No preview while there is an error." : "No preview while there are errors."]);
    return;
  }
  draw(data);
}

// "It gets bb06, the next free signal." from Rust, which knows every signal
// the folder's apps send.
function showNext(signal) {
  state.next = signal;
  $("next-hint").textContent = signal ? `It gets ${signal}, the next free signal.` : "";
  decorate();
}

function showReport(report) {
  const live = report.warnings.filter((w) => !w.dismissed).length;
  showCount($("count-errors"), report.errors.length, "error", "bad");
  showCount($("count-warnings"), live, "warning", "warn");
  $("findings").replaceChildren(
    ...report.errors.map((f) => findingCard(f, "error")),
    ...report.warnings.map((f) => findingCard(f, f.dismissed ? "dismissed" : "warning")),
    ...report.settings.map((f) => findingCard(f, "setting")),
  );
  showStatus();
  decorate();
}

function showCount(chip, n, noun, tone) {
  chip.textContent = counted(n, noun);
  chip.className = n ? `count ${tone}` : "count";
}

// A card for each error and warning: its title, then what it says in full.
// A warning can be dismissed, and then it stays in the list, dimmed. A
// setting card, in a neutral tone, shows what a setting the user chose does,
// such as smaller text. It is not advice, so it has no Dismiss, and no count
// includes it. A card can offer a button of its own that changes an app
// setting, as the App pane's controls do: Allow smaller text, or Use 25 px.
// It has a row to itself, above the row every warning has. A dismissed card
// keeps it, so smaller text can always be turned on.
function findingCard(f, kind) {
  const offer = f.action
    ? el("div.finding-actions", {}, el("button.btn.small", { onclick: () => {
      state.layout.smallText = f.action.smallText;
      schedule();
    } }, f.action.label))
    : null;
  const actions = [];
  if (kind === "warning") {
    actions.push(el("button.btn.small.dim", { onclick: () => {
      state.layout.dismissed = [...(state.layout.dismissed || []), f.key];
      schedule({ keepBuild: true });
    } }, "Dismiss"));
  }
  if (kind !== "dismissed" && f.button !== null && f.button !== undefined) {
    // A click from the keyboard has no click count.
    const shows = el("button.btn.small", { onclick: (event) => showButton(f.button, event.detail === 0) },
      icon("arrow-right"), "Show button");
    if (kind === "warning") shows.classList.add("dim");
    actions.push(shows);
  }
  return el(`div.finding.${kind}`, {},
    icon({ error: "circle-x", setting: "info" }[kind] || "triangle-alert"),
    el("div.finding-text", {},
      el("p.finding-title", {}, kind === "dismissed" ? `${f.title} (dismissed)` : f.title),
      el("p.finding-body", {}, f.message),
      offer,
      actions.length ? el("div.finding-actions", {}, ...actions) : null));
}

// The line under the checks: what stops the build, or that nothing does.
function showStatus() {
  const n = state.errors;
  const words = n
    ? `Fix the ${n === 1 ? "error" : "errors"} to build.`
    : state.built ? `No errors. Built at ${clock(state.built)}.` : "No errors. Ready to build.";
  const status = $("check-status");
  status.className = `check-status ${n ? "bad" : "good"}`;
  status.replaceChildren(icon(n ? "octagon-x" : "circle-check"), el("span", {}, words));
  showDrawerHead();
}

// 14:02, as the window shows a time.
function clock(date) {
  return [date.getHours(), date.getMinutes()].map((n) => String(n).padStart(2, "0")).join(":");
}

function draw(data) {
  imageUrls.forEach((url) => URL.revokeObjectURL(url));
  imageUrls = [];
  const urls = {};
  for (const [name, bytes] of Object.entries(data.images)) {
    const url = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: "image/png" }));
    urls[name] = url;
    imageUrls.push(url);
  }
  state.art = urls;
  paintThumbs();

  const run = window.BandPreview.runPage(data.page);
  // What each button sends, from a second run of the page pressed button by
  // button, so the first run keeps only what the user sends. Two buttons that
  // send one signal get a red ring, and a line under the screen.
  const probe = window.BandPreview.runPage(data.page);
  const shared = sharedSignals(probe.widgets.map((_, i) => probe.press(i)));
  const ringed = new Set([...shared.values()].flat());
  const nodes = run.widgets.map((w, i) => {
    const p = w.props;
    const node = document.createElement("div");
    node.className = "widget";
    Object.assign(node.style, {
      left: p.x + "px", top: p.y + "px", width: p.w + "px", height: p.h + "px",
      borderRadius: (p.radius || 0) + "px", fontSize: (p.text_size || 20) + "px",
    });
    // While a finger is on a button, the band draws its pressed picture or
    // its pressed color, and so does the preview while the pointer is down.
    const look = (down) => {
      const image = [down && p.press_src, p.normal_src, p.src].find((name) => name && urls[name]);
      const color = down && p.press_color !== undefined ? p.press_color : p.normal_color;
      node.style.background = "";
      if (image) node.style.backgroundImage = `url(${urls[image]})`;
      else if (color !== undefined) node.style.background = hexColor(color);
      else if (w.type === "FILL_RECT" && p.color !== undefined) node.style.background = hexColor(p.color);
    };
    look(false);
    node.onpointerdown = () => look(true);
    node.onpointerup = node.onpointerleave = node.onpointercancel = () => look(false);
    node.textContent = p.text || "";
    node.classList.toggle("shared", ringed.has(i));
    node.onclick = () => {
      showSent(run.press(i));
      for (const other of nodes) other.classList.toggle("last", other === node && !ringed.has(i));
      showPreviewProblems(run, shared);
    };
    return node;
  });
  $("screen").replaceChildren(...nodes);
  showStrip();
  showSent(undefined);
  showPreviewProblems(run, shared);
}

// Each signal that two or more buttons send, with the buttons that send it.
function sharedSignals(sends) {
  const by = new Map();
  sends.forEach((signal, i) => {
    if (signal) by.set(signal, [...(by.get(signal) || []), i]);
  });
  return new Map([...by].filter(([, buttons]) => buttons.length > 1));
}

// What the last click sent: a signal, nothing, or no click yet.
function showSent(signal) {
  $("sent").hidden = !signal;
  $("sent-signal").textContent = signal || "";
  $("sent-hint").hidden = Boolean(signal);
  $("sent-hint").textContent = signal === null ? "This button sent nothing." : "Click a button to see its signal";
}

// The line that marks where the free strip starts, and its size.
function showStrip() {
  const px = state.layout.bottomStrip;
  const mark = $("strip-mark");
  mark.hidden = px <= 0;
  mark.style.top = `${(SCREEN_H - Math.min(px, SCREEN_H)) * SCALE}px`;
  $("strip-label").textContent = `Free strip · ${px} px`;
  // The words sit under the line, so a thin strip has no room for them.
  $("strip-label").hidden = px * SCALE < 27;
}

function showPreviewProblems(run, shared) {
  const lines = run.errors.map((e) => "The page failed. On the band this is a black screen.\n" + e)
    .concat(run.unknown.map((t) => "The preview cannot draw a " + t + " widget."))
    .concat([...shared].map(([signal, buttons]) =>
      `${buttonNames(buttons)} ${buttons.length === 2 ? "both" : "all"} send ${signal}.`));
  showProblems(lines);
}

function showProblems(lines) {
  $("preview-problems").replaceChildren(...(lines.length
    ? lines.map((line) => el("p.problem", {}, icon("circle-x"), el("span", {}, line)))
    : [el("p.no-problems", {}, icon("check"), "No preview problems")]));
}

// “Next” and “Blank”, or Button 2 and button 4 when they have no labels.
function buttonNames(buttons) {
  const names = buttons.map((i, n) => {
    const label = (state.layout.buttons[i] || {}).label;
    return label && label.trim() ? `“${label.trim()}”` : `${n ? "button" : "Button"} ${i + 1}`;
  });
  return names.length > 2
    ? `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`
    : names.join(" and ");
}

// BandBridge's state in the title bar (decision E3). The page asks every
// five seconds while the window is shown, and at once when it is shown
// again. A busy BandBridge, or a failed ask, leaves the pill as it was.
const BRIDGE_EVERY = 5000;
const BRIDGE_PILL = {
  connected: { tone: "good", words: "Mi Band Bridge · band connected" },
  not_connected: { tone: "warn", words: "Mi Band Bridge · band not connected" },
  not_running: { tone: "idle", words: "Mi Band Bridge is not running" },
};
const bridgeAsk = { timer: null, asking: false };

async function askBridge() {
  clearTimeout(bridgeAsk.timer);
  bridgeAsk.timer = null;
  // An answer is on its way, and it sets the next ask.
  if (bridgeAsk.asking) return;
  bridgeAsk.asking = true;
  try {
    showBridge(await invoke("bridge_status"));
  } catch (e) {
    // The pill keeps what it showed.
  } finally {
    bridgeAsk.asking = false;
  }
  if (!document.hidden) bridgeAsk.timer = setTimeout(askBridge, BRIDGE_EVERY);
}

function showBridge(status) {
  const pill = BRIDGE_PILL[status];
  if (!pill) return;
  // Install on band follows the band, within one ask.
  if (state.bridge !== status) {
    state.bridge = status;
    updateToolbar();
  }
  const chip = $("bridge");
  chip.dataset.tone = pill.tone;
  // The pill is a status region, so it only changes when the state does.
  if ($("bridge-state").textContent !== pill.words) $("bridge-state").textContent = pill.words;
  chip.hidden = false;
}

document.addEventListener("visibilitychange", () => {
  if (document.hidden) {
    clearTimeout(bridgeAsk.timer);
    bridgeAsk.timer = null;
  } else {
    askBridge();
  }
});

const pickFolder = guarded(async () => {
  const folder = await invoke("pick_folder");
  if (folder) await openFolder(folder);
});
$("folder").onclick = pickFolder;
$("start-folder").onclick = pickFolder;
$("apps").onchange = guarded(() => openApp($("apps").value));
$("new-app").onclick = guarded(async () => {
  const dir = await invoke("create_app", { folder: state.folder, name: $("new-name").value });
  $("new-name").value = "";
  await openFolder(state.folder, dir);
});
$("name").oninput = () => {
  state.layout.name = $("name").value;
  // The picker names the app as its layout does.
  const chosen = $("apps").selectedOptions[0];
  if (chosen) chosen.text = state.layout.name;
  showAppName();
  schedule();
};
$("columns").onclick = (event) => {
  const segment = event.target.closest("button");
  if (!segment) return;
  state.layout.columns = Number(segment.dataset.value);
  fillForm();
  schedule();
};
$("corners").onclick = (event) => {
  const segment = event.target.closest("button");
  if (!segment) return;
  state.layout.corners = segment.dataset.value;
  choose($("corners"), state.layout.corners);
  schedule();
};
$("strip").oninput = () => {
  const px = Math.round(Number($("strip").value));
  if (!Number.isFinite(px) || $("strip").value === "") return;
  state.layout.bottomStrip = Math.max(0, Math.min(MAX_STRIP, px));
  $("strip-slider").value = state.layout.bottomStrip;
  $("strip-slider").style.setProperty("--at", String(state.layout.bottomStrip / MAX_STRIP));
  schedule();
};
// Left empty or out of range, the field shows the strip the layout holds.
$("strip").onchange = () => showStripValue(state.layout.bottomStrip);
$("strip-slider").oninput = () => setStrip(Number($("strip-slider").value));
$("icon-pick").onclick = guarded(async () => {
  const rel = await invoke("pick_image", { appDir: state.appDir });
  if (rel) { state.layout.icon = rel; fillForm(); schedule(); }
});
$("icon-clear").onclick = () => { state.layout.icon = null; fillForm(); schedule(); };
$("add-button").onclick = guarded(async () => {
  const l = state.layout;
  const signal = await invoke("next_signal", { appDir: state.appDir, layout: l });
  l.buttons.push({ label: "", signal: signal || "", color: l.color, pressColor: null, image: null, span: false });
  // The new button's row opens, ready for its label.
  showButton(l.buttons.length - 1);
  rowOf(l.buttons.length - 1).querySelector(".label-input").focus();
  schedule();
});
$("drawer-toggle").onclick = () => setDrawer(!drawer.open);
$("log").addEventListener("scroll", () => {
  const log = $("log");
  drawer.follow = log.scrollHeight - log.scrollTop - log.clientHeight < 8;
});
window.addEventListener("resize", followLog);
$("drawer-show").onclick = () => setDrawer(true);
// Clear empties the drawer: its lines, and the card of an install that
// stopped. A run that goes on keeps its chip, and its next line starts the
// log again.
$("drawer-clear").onclick = () => {
  $("log").replaceChildren();
  $("log").hidden = true;
  drawer.progress = null;
  drawer.follow = true;
  if (!drawer.run) drawer.ended = null;
  showStopCard();
  showDrawerHead();
};

// Build: save and check what is on screen, then build it, with each step in
// the drawer as it happens. The run starts first, so a second click finds
// it. The build saves the app, with its version raised when zmake made it,
// so the form loads it again afterwards.
$("build").onclick = guarded(async () => {
  if (drawer.run || !state.appDir) return;
  const appDir = state.appDir;
  beginRun("build", appDir);
  let built = { ok: false, at: Date.now() };
  try {
    clearTimeout(timer);
    timer = null;
    await refresh();
    built = await invoke("build_app", { appDir, layout: state.layout });
  } finally {
    endRun(built.ok ? "built" : "stopped");
  }
  if (state.appDir !== appDir) return;
  state.layout = await invoke("load_layout", { appDir });
  fillForm();
  if (built.ok) state.built = new Date(built.at);
  showStatus();
});

// Install on band sends the built app to BandBridge, with each step, and
// the upload's progress, as they happen.
async function install(appDir) {
  if (drawer.run || !appDir) return;
  beginRun("install", appDir);
  let how = "stopped";
  try {
    // What is on screen is saved first, as it is for a build.
    if (appDir === state.appDir) {
      clearTimeout(timer);
      timer = null;
      await refresh();
    }
    how = await invoke("install_app", { appDir });
  } finally {
    endRun(how);
  }
}
$("install").onclick = guarded(() => install(state.appDir));
// Again means the install that stopped, even with another app open now.
$("install-again").onclick = guarded(() => install(drawer.ended ? drawer.ended.appDir : state.appDir));

// State 4 (decision E8): the build tools notice, with each tool found or
// missing and the commands for this system, from tools_status. Nothing else
// works until the tools are there, so the toolbar is off, and no folder
// opens.
function showTools(tools) {
  state.toolsMissing = true;
  state.steps = tools.steps;
  $("tools-system").textContent = `On this ${tools.system}`;
  // On Windows the commands are PowerShell's own, so the notice says to run
  // them there, and shows PowerShell's prompt in front of each.
  const shell = tools.shell || "terminal";
  const prompt = shell === "PowerShell" ? "PS>" : "$";
  $("tools-shell").textContent = `Run in ${shell}`;
  $("tool-rows").replaceChildren(...tools.tools.map((tool) =>
    el(`div.tool-row.${tool.found ? "found" : "missing"}`, {},
      icon(tool.found ? "circle-check" : "circle-x"),
      el("span.tool-name", {}, tool.name),
      el("span.tool-detail", { title: tool.detail }, tool.detail),
      el("span.tool-state", {}, tool.found ? "Found" : "Missing"))));
  $("tool-steps").replaceChildren(...tools.steps.map((step) =>
    el("div.cmd-line", {}, el("span.prompt", { "aria-hidden": "true" }, prompt), el("code.cmd", {}, step))));
  showView("tools");
  updateToolbar();
}

// Copy puts the commands on the clipboard, one to a line, without the prompt.
// Its word says it did, for two seconds.
let copied = null;
$("copy-steps").onclick = async () => {
  const done = await copyText(state.steps.join("\n"));
  $("copy-words").textContent = done ? "Copied" : "Copy";
  clearTimeout(copied);
  copied = setTimeout(() => { $("copy-words").textContent = "Copy"; }, 2000);
};

// The clipboard, or, where the page may not use it, a copy of a selection.
async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch (e) {
    const area = el("textarea", { value: text, readOnly: true });
    area.style.cssText = "position: fixed; top: 0; left: 0; opacity: 0;";
    document.body.append(area);
    area.select();
    const done = document.execCommand("copy");
    area.remove();
    return done;
  }
}

$("quit").onclick = guarded(() => invoke("quit"));

// --- the caption buttons (Windows) ------------------------------------------
// On Windows the window has no frame (W8), so the page draws Windows' buttons
// at the top right, as BandBridge's window does: minimize, maximize or
// restore, and close, which quits the editor. A double click on the bar
// maximizes too, through the bar's own drag region. The middle button shows
// what it does now, so the page asks each time the window's size changes.
// Windows dims the buttons of a window that is not in front, and so does the
// page.
if (document.documentElement.dataset.system === "windows") {
  const frame = tauri.window && tauri.window.getCurrentWindow
    ? tauri.window.getCurrentWindow()
    : null;
  const ask = (call) => frame
    ? frame[call]().catch((e) => console.error(`the window did not ${call}`, e))
    : Promise.resolve();
  const maximize = $("caption-maximize");
  const drawMaximize = (maximized) => {
    const words = maximized ? "Restore down" : "Maximize";
    maximize.textContent = maximized ? "" : "";
    maximize.title = words;
    maximize.setAttribute("aria-label", words);
  };
  const askMaximized = () => {
    if (frame) frame.isMaximized().then(drawMaximize).catch(() => {});
  };
  $("caption-minimize").onclick = () => ask("minimize");
  maximize.onclick = () => ask("toggleMaximize").then(askMaximized);
  $("caption-close").onclick = () => ask("close");
  // Windows' caption buttons never take the keyboard's focus, so a field
  // being typed in keeps it.
  for (const button of document.querySelectorAll("button.caption")) {
    button.addEventListener("mousedown", (event) => event.preventDefault());
  }
  const drawInFront = () => {
    document.documentElement.classList.toggle("window-inactive", !document.hasFocus());
  };
  window.addEventListener("resize", askMaximized);
  window.addEventListener("focus", drawInFront);
  window.addEventListener("blur", drawInFront);
  askMaximized();
  drawInFront();
}

// Each line a build or an install says, as it happens (decision E4).
if (tauri.event && tauri.event.listen) tauri.event.listen("output", (event) => addLine(event.payload));

start().catch((e) => {
  show(String(e));
  // Whatever stopped the start, the window still offers a folder to open.
  if ($("main").hidden) showView("start");
});

// The About dialog: this build's version, and the licenses and notices it
// opens in the browser, each by its name.
$("about").onclick = () => {
  $("about-note").hidden = true;
  $("about-dialog").showModal();
};
invoke("version")
  .then((version) => { $("about-version").textContent = version; })
  .catch(() => {});
for (const link of $("about-dialog").querySelectorAll("[data-page]")) {
  link.onclick = () => {
    $("about-note").hidden = true;
    invoke("open_about_page", { name: link.dataset.page }).catch((e) => {
      $("about-note").textContent = String(e);
      $("about-note").hidden = false;
    });
  };
}
