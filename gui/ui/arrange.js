// The apps and the band's screen lists, as the design frame "BandBridge
// Window — Light" draws its sections "Install or remove an app" and
// "Arrange what the device shows".
//
// The band sends each screen as an id, with the name Gadgetbridge gives a
// built-in screen (src/menu_names.rs), or an app's name, and the apps as
// ids with the names this Mac learned when it installed them. This file
// says what a person reads for each and which icon goes with it, and
// whether a move in a list can be made. It touches no page, so
// gui/tests/arrange.rs runs it under Node.

// The BandBridge band app (band-app/app.json). The lists mark it with the
// window's own mark, as the design does.
const BANDBRIDGE_APP = "58E537B5";

// Each built-in screen, by the band's name for it: its words and its icon,
// from the Lucide set the page draws. The same name means the same screen
// in the app menu and in the shortcuts.
const SCREENS = {
  activity: ["Activity", "flame"],
  alarm: ["Alarm", "alarm-clock"],
  alexa: ["Alexa", "mic"],
  alexa2: ["Alexa", "mic"],
  alipay: ["Alipay", "wallet"],
  apps_shortcuts: ["App shortcuts", "layout-grid"],
  audio_control: ["Audio control", "headphones"],
  barometer: ["Barometer", "mountain"],
  bio_charge: ["BioCharge", "zap"],
  bluetooth_scale_assistant: ["Scale assistant", "weight"],
  body_composition: ["Body composition", "scale"],
  breathing: ["Breathing", "leaf"],
  breathing2: ["Breathing", "leaf"],
  calculator: ["Calculator", "calculator"],
  calendar: ["Calendar", "calendar"],
  calorie_intake_record: ["Calorie log", "utensils"],
  cards: ["Cards", "credit-card"],
  compass: ["Compass", "compass"],
  countdown: ["Countdown", "hourglass"],
  diary: ["Diary", "notebook-pen"],
  eventreminder: ["Event reminders", "calendar-clock"],
  events: ["Events", "calendar-days"],
  female_health: ["Cycle tracking", "flower-2"],
  findphone: ["Find phone", "smartphone"],
  flashlight: ["Flashlight", "flashlight"],
  golf: ["Golf", "flag"],
  heart_rate_push: ["Heart rate sharing", "heart-pulse"],
  hr: ["Heart rate", "heart"],
  hrv: ["HRV", "heart-pulse"],
  intervals_icu: ["Intervals.icu", "activity"],
  jet_lag_manager: ["Jet lag", "plane"],
  map: ["Map", "map"],
  meditation: ["Meditation", "brain"],
  membership_cards: ["Membership cards", "credit-card"],
  mi_ai: ["Mi AI", "sparkles"],
  music: ["Music", "music"],
  mutephone: ["Mute phone", "bell-off"],
  offline_voice: ["Offline voice", "mic"],
  one_tap_measuring: ["One-tap measure", "scan-heart"],
  pai: ["PAI", "trending-up"],
  personal_activity_intelligence: ["Personal Activity Intelligence", "trending-up"],
  phone: ["Phone", "phone"],
  podcast: ["Podcasts", "podcast"],
  pomodoro: ["Pomodoro", "clock"],
  readiness: ["Readiness", "battery-charging"],
  real_time_heart_rate: ["Live heart rate", "heart-pulse"],
  running_calculator: ["Running calculator", "calculator"],
  settings: ["Settings", "settings"],
  sleep: ["Sleep", "moon"],
  spo2: ["SpO2", "wind"],
  stopwatch: ["Stopwatch", "timer"],
  stress: ["Stress", "gauge"],
  sun_moon: ["Sun and moon", "sun-moon"],
  takephoto: ["Camera", "camera"],
  takephoto2: ["Camera", "camera"],
  thermometer: ["Thermometer", "thermometer"],
  todo: ["To-do list", "list-todo"],
  training_calendar: ["Training calendar", "calendar-range"],
  voice_memos: ["Voice memos", "mic"],
  watch_storage_space: ["Storage", "hard-drive"],
  water_time: ["Water reminder", "glass-water"],
  weather: ["Weather", "cloud-sun"],
  wechat_pay: ["WeChat Pay", "wallet"],
  workout: ["Workout", "activity"],
  workout_history: ["Workout history", "history"],
  workout_shortcuts: ["Workout shortcuts", "dumbbell"],
  workout_status: ["Workout status", "dumbbell"],
  worldclock: ["World clock", "globe"],
  worldclock2: ["World clock", "globe"],
  zepp_coach: ["Zepp Coach", "dumbbell"],
  zepp_flow: ["Zepp Flow", "sparkles"],
  zepp_flow2: ["Zepp Flow", "sparkles"],
  zepp_pay: ["Zepp Pay", "wallet"],
};

// A screen the table does not know yet: its own name, as words.
const OTHER_SCREEN_ICON = "layout-grid";

// The two lists the window arranges, and what each allows. The band takes
// at most ten shortcuts, and keeps Settings in its app menu (measured on the
// band: docs/DEVELOPER.md, "screens"). The control center is not in the
// window (D5).
const LISTS = {
  menu: { title: "App menu", name: "the app menu", most: null, keeps: "settings" },
  shortcuts: { title: "Shortcuts", name: "the shortcuts", most: 10, keeps: null },
};

// What a person reads for a screen: an app by its name, which is its id
// when this Mac has none, and a built-in screen by its words.
function screenLabel(entry) {
  if (entry.is_app) return entry.name || entry.id;
  const known = SCREENS[entry.name];
  if (known) return known[0];
  const words = String(entry.name || entry.id).replace(/_/g, " ");
  return words.charAt(0).toUpperCase() + words.slice(1);
}

// The icon for a screen: the window's mark for the BandBridge band app, a
// box for any other app, and a built-in screen's own.
function screenIcon(entry) {
  if (entry.is_app) return String(entry.id).toUpperCase() === BANDBRIDGE_APP ? "watch" : "box";
  const known = SCREENS[entry.name];
  return known ? known[1] : OTHER_SCREEN_ICON;
}

// Whether a screen gets the custom tag: an app installed from here, which
// this Mac knows by name.
function isCustom(entry) {
  return entry.is_app === true && entry.installed_here === true;
}

// The tooltip for a screen: its words, and its id, which tells apart two
// screens with the same words.
function screenTitle(entry) {
  return `${screenLabel(entry)} (${entry.id})`;
}

// The screens the band shows, in its order.
function shownEntries(entries) {
  return entries.filter((entry) => entry.shown);
}

// The screens the band keeps out of view, by their words. Their order on
// the band means nothing to a person.
function hiddenEntries(entries) {
  return entries
    .filter((entry) => !entry.shown)
    .sort((a, b) => screenLabel(a).localeCompare(screenLabel(b)) || a.id.localeCompare(b.id));
}

// "6 shown · 2 hidden", in a column's head.
function listCounts(entries) {
  const shown = shownEntries(entries).length;
  return `${shown} shown · ${entries.length - shown} hidden`;
}

// Where a screen can go, one step at a time, for the keyboard: each place
// among the others shown, from the top, then the hidden group.
function placesFor(entries, id) {
  const others = shownEntries(entries).filter((entry) => entry.id !== id).length;
  return [...Array(others + 1).keys(), "hidden"];
}

// Where a screen is now, in the terms of placesFor.
function placeOf(entries, id) {
  const at = shownEntries(entries).findIndex((entry) => entry.id === id);
  return at === -1 ? "hidden" : at;
}

// What moving screen `id` to `to` asks of the band: `to` is a place among
// the other shown screens, counted from the top, or "hidden". The answer is
// {shown}, the shown screens' ids in their new order, or {refused}, the
// words that say why the band would not take it, or null when nothing
// changes.
function moveTo(list, entries, id, to) {
  const rules = LISTS[list];
  const moving = entries.find((entry) => entry.id === id);
  if (!rules || !moving) return null;
  const before = shownEntries(entries).map((entry) => entry.id);
  const others = before.filter((shown) => shown !== id);
  let shown;
  if (to === "hidden") {
    if (!moving.shown) return null;
    if (rules.keeps && !moving.is_app && moving.name === rules.keeps) {
      return { refused: `The device keeps ${screenLabel(moving)} in its app menu.` };
    }
    shown = others;
  } else {
    const at = Math.max(0, Math.min(Number(to), others.length));
    shown = [...others.slice(0, at), id, ...others.slice(at)];
    if (rules.most !== null && shown.length > rules.most) {
      return { refused: `The device shows at most ${rules.most} shortcuts. Hide one first.` };
    }
  }
  if (shown.length === before.length && shown.every((shownId, i) => shownId === before[i])) {
    return null;
  }
  return { shown };
}

// The entries once the band shows `shown`, in that order: the list as a
// move leaves it, drawn before the band answers.
function arranged(entries, shown) {
  const byId = new Map(entries.map((entry) => [entry.id, entry]));
  const front = shown.map((id, position) => ({ ...byId.get(id), shown: true, position }));
  const rest = entries
    .filter((entry) => !shown.includes(entry.id))
    .map((entry) => ({ ...entry, shown: false }));
  return [...front, ...rest];
}

// Whether the band shows what was asked, in that order.
function showsAsAsked(entries, shown) {
  const now = shownEntries(entries).map((entry) => entry.id);
  return now.length === shown.length && now.every((id, i) => id === shown[i]);
}

// Where a screen being moved would go, in words, for a person who moves it
// with the keyboard.
function placeWords(list, entries, id, to) {
  const moving = entries.find((entry) => entry.id === id);
  const label = moving ? screenLabel(moving) : id;
  if (to === "hidden") return `${label}: hidden.`;
  const count = shownEntries(entries).filter((entry) => entry.id !== id).length + 1;
  return `${label}: place ${Number(to) + 1} of ${count} in ${LISTS[list].name}.`;
}

// Why the band task could not do something, without the words that say
// which part of it failed, which the window says itself:
// "the band is not connected. Connect first."
function reasonOf(error) {
  return String(error || "").replace(/^(install failed|apps|display list): /, "");
}

// What a list's box says while the band is not there to read it: `where`
// is "coming" while it connects or tries again, and "away" when nobody is
// reaching for it. `what` is the list, such as "its apps".
function awayWords(where, what) {
  if (where === "coming") return `Waiting for the device to connect, to read ${what}.`;
  return `Connect the device to see ${what}.`;
}

// --- the apps ---------------------------------------------------------------

// An installed app's row: its name, and its id and version in the line
// under it, as "58E537B5 · v1.0.12". The window lists only the apps it can
// name, so a name is always there, and the id stands in if not.
function appRow(app) {
  const version = String(app.version || "");
  return {
    name: app.name || app.id,
    id: app.id,
    version: version === "" || /^v/i.test(version) ? version : `v${version}`,
  };
}

// The apps list with no app in it.
function appsEmptyWords() {
  return "No apps installed from here yet.";
}

// The question before a removal, in the row.
function removeQuestion(app) {
  return `Remove “${appRow(app).name}” from the device? To put it back, install its .bin again.`;
}

// The file's own name, from the path the picker gave.
function packageName(path) {
  const parts = String(path || "").split(/[\\/]/);
  return parts[parts.length - 1] || String(path || "");
}

// What the install says as it goes, and when it ends: its words and tone.
function installWords(stage, path, error) {
  const file = packageName(path);
  if (stage === "running") {
    return {
      tone: "busy",
      words: `Installing ${file}. The device stays connected until the app is on, then connects again.`,
    };
  }
  if (stage === "done") return { tone: "good", words: `Installed ${file}.` };
  return { tone: "bad", words: `${file} was not installed: ${reasonOf(error)}` };
}
