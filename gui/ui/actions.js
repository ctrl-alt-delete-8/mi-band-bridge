// The actions section's words and data, as the design frame "BandBridge
// Window — Light" draws them in its section "What each button does", and as
// the frame "State variations — Action kinds" draws the editor's four tabs.
//
// An action comes from actions_list in the file's own shape: a name, a
// signal, and one kind key, such as {"name": "Open Music", "signal": "bb03",
// "shell": "open -a Music"}. The editor sends it back in the same shape. This
// file says what the list shows for each action, reads an action into the
// editor and back out, names the keys a keystroke holds, and says where each
// problem the file's rules find goes. It touches no page, so
// gui/tests/action_editor.rs runs it under Node.

// The kinds the file knows, in its order, with the words the list gives them.
const KIND_WORDS = {
  keystroke: "Keystroke",
  shell: "Shell",
  script: "Script",
  shortcut: "Shortcut",
  web_request: "Web request",
};

// The editor's tabs, in the design's order. A script is on the Shell tab.
const TABS = ["keystroke", "shell", "shortcut", "web_request"];

// The methods a web request takes, as the file's rules list them.
const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"];

// The modifiers a keystroke holds, in the order the key list gives them,
// with the mark a key cap shows for each on a Mac. A PC's caps show the
// labels key_names gives instead.
const MODIFIER_MARKS = { command: "⌘", shift: "⇧", option: "⌥", control: "⌃", fn: "fn" };

// The keys that are modifiers in their own right. Record keys waits past
// them for the key they go with.
const MODIFIER_KEYS = [
  "left_control", "left_option", "left_shift", "left_command",
  "right_control", "right_option", "right_shift", "right_command",
  "caps_lock", "fn",
];

// Until key_names answers: the modifiers in the list's order, and no other
// spellings, so a keystroke still reads as its own words.
const NO_NAMES = {
  keys: [],
  mediaKeys: [],
  modifiers: ["command", "shift", "option", "control", "fn"],
  aliases: { keys: {}, mediaKeys: {}, modifiers: {} },
};

// The kind an action has: the one kind key it holds, or null.
function kindOf(action) {
  return Object.keys(KIND_WORDS).find((kind) => action && kind in action) || null;
}

// The name a group lists `name` under: the name it is another spelling of,
// or the name itself.
function listedAs(aliases, name) {
  return (aliases && aliases[name]) || name;
}

// Three modifiers have shorter names, which the file takes too.
const SHORT_MODIFIERS = { cmd: "command", alt: "option", ctrl: "control" };

// The name the list gives a modifier: left_command, right_cmd and cmd are
// all command.
function modifierListed(modifier, names) {
  const known = names || NO_NAMES;
  if (known.aliases.modifiers[modifier]) return known.aliases.modifiers[modifier];
  const bare = modifier.replace(/^(left|right)_/, "");
  return SHORT_MODIFIERS[bare] || bare;
}

// The modifiers a keystroke holds, by the names the list gives them, each
// once, in the file's press order: ["left_shift", "cmd"] is
// ["command", "shift"]. This is the order the file keeps and the core sends.
function modifiersHeld(modifiers, names) {
  const known = names || NO_NAMES;
  const held = new Set((modifiers || []).map((modifier) => modifierListed(modifier, known)));
  return known.modifiers.filter((modifier) => held.has(modifier));
}

// The Windows order for the caps, the words and the chips: Ctrl, Alt, Shift,
// Win, as Windows writes Ctrl+Shift+Esc. The file keeps the press order
// (command, shift, option, control); only the display changes (W2).
const WINDOWS_MODIFIER_ORDER = ["control", "option", "shift", "command", "fn"];

// The modifiers a keystroke holds, in the order to show them: the file's
// press order on a Mac, and the Windows order on a PC. Display only, so the
// file and the core keep the press order.
function modifiersShown(modifiers, names) {
  const held = modifiersHeld(modifiers, names);
  if (!pcKeyboard(names)) return held;
  const set = new Set(held);
  return WINDOWS_MODIFIER_ORDER.filter((modifier) => set.has(modifier));
}

// Whether the names are a PC keyboard's: key_names gives labels for the
// modifiers there, Ctrl, Alt, Shift and Win (W2), and none on a Mac.
function pcKeyboard(names) {
  return Boolean(names && names.labels && Object.keys(names.labels).length > 0);
}

// The word for a modifier: its label on a PC, and its name on a Mac.
function modifierWord(modifier, names) {
  return (names && names.labels && names.labels[modifier]) || modifier;
}

// A keystroke in words, as the list shows it: "command shift 4", or on a PC
// "Win Shift 4", or a media key's name, "volume_increment". Empty for a
// keystroke with no key yet.
function keysWords(keystroke, names) {
  if (!keystroke) return "";
  const known = names || NO_NAMES;
  if (keystroke.consumer_key_code) {
    return listedAs(known.aliases.mediaKeys, keystroke.consumer_key_code);
  }
  const key = keystroke.key_code ? listedAs(known.aliases.keys, keystroke.key_code) : "";
  const held = modifiersShown(keystroke.modifiers, known).map((modifier) => modifierWord(modifier, known));
  return [...held, key].filter(Boolean).join(" ");
}

// The method a web request sends: the one it names, or POST when it has a
// body, and GET otherwise, as the file's rules read it.
function methodOf(request) {
  if (request.method) return request.method.trim().toUpperCase();
  return request.body !== undefined && request.body !== null ? "POST" : "GET";
}

// An address without its scheme, as the design's list writes it.
function shownUrl(url) {
  return String(url || "").replace(/^https?:\/\//i, "");
}

// What the list shows for one action: its signal, its name, its kind, and
// what it does, in the mono face unless it is a shortcut's name, with the
// words in full on the tooltip. `listed` is one entry of actions_list.
function listRow(listed, names) {
  const action = listed.action;
  const kind = listed.kind || kindOf(action);
  const row = {
    signal: action.signal,
    name: action.name,
    kind: KIND_WORDS[kind] || kind,
    value: "",
    mono: true,
    title: "",
  };
  if (kind === "keystroke") {
    row.value = keysWords(action.keystroke, names);
  } else if (kind === "shell" || kind === "script") {
    row.value = action[kind];
  } else if (kind === "shortcut") {
    row.value = action.shortcut;
    row.mono = false;
  } else if (kind === "web_request") {
    const method = methodOf(action.web_request);
    row.value = `${method} ${shownUrl(action.web_request.url)}`;
    row.title = `${method} ${action.web_request.url}`;
  }
  row.title = row.title || row.value;
  return row;
}

// The count in the section's head: "4 set".
function countWords(count) {
  return count === 0 ? "none set" : `${count} set`;
}

// --- key caps ---------------------------------------------------------------

// The words a cap shows for a key, by its name in the list.
const KEY_LABELS = {
  return_or_enter: "return", tab: "tab", spacebar: "space",
  delete_or_backspace: "delete", delete_forward: "fwd delete", escape: "esc",
  up_arrow: "↑", down_arrow: "↓", left_arrow: "←", right_arrow: "→",
  page_up: "page up", page_down: "page down", caps_lock: "caps lock",
  hyphen: "-", equal_sign: "=", open_bracket: "[", close_bracket: "]",
  backslash: "\\", semicolon: ";", quote: "'", grave_accent_and_tilde: "`",
  comma: ",", period: ".", slash: "/",
};

// The words a cap shows for a media key.
const MEDIA_LABELS = {
  volume_increment: "volume up", volume_decrement: "volume down", mute: "mute",
  play_or_pause: "play/pause", scan_next_track: "next track",
  scan_previous_track: "previous track", fast_forward: "fast forward",
  rewind: "rewind", eject: "eject",
  display_brightness_increment: "brightness up",
  display_brightness_decrement: "brightness down",
  illumination_increment: "key light up", illumination_decrement: "key light down",
};

// The words a cap shows for a key on a PC keyboard (W2), as a PC keyboard
// prints them. Arrows and symbols read as on a Mac, and any other key is its
// name, each word capitalized: Home, Insert, Scroll Lock, Keypad 1.
const PC_KEY_LABELS = {
  return_or_enter: "Enter", tab: "Tab", spacebar: "Space",
  delete_or_backspace: "Backspace", delete_forward: "Delete", escape: "Esc",
  page_up: "Page Up", page_down: "Page Down", caps_lock: "Caps Lock",
  keypad_num_lock: "Num Lock", print_screen: "Print Screen",
  // The key that opens a context menu, which Windows calls the Menu key.
  application: "Menu",
};

// The words a cap shows for a media key on a PC keyboard.
const PC_MEDIA_LABELS = {
  volume_increment: "Volume Up", volume_decrement: "Volume Down", mute: "Mute",
  play_or_pause: "Play/Pause", scan_next_track: "Next Track",
  scan_previous_track: "Previous Track",
};

// A word with its first letter in capitals.
function capitalized(word) {
  return word.charAt(0).toUpperCase() + word.slice(1);
}

// The words a cap shows for a key: a letter in capitals, F5 for f5, an
// arrow as an arrow, and the rest as the list names them, in words. On a PC,
// the words a PC keyboard prints, and a modifier key by its label, such as
// Left Win.
function keyLabel(name, names) {
  if (/^[a-z]$/.test(name)) return name.toUpperCase();
  if (/^f\d+$/.test(name)) return name.toUpperCase();
  if (!pcKeyboard(names)) {
    if (KEY_LABELS[name]) return KEY_LABELS[name];
    return name.replace(/_/g, " ");
  }
  if (PC_KEY_LABELS[name]) return PC_KEY_LABELS[name];
  const side = /^(left|right)_(command|option|control|shift)$/.exec(name);
  if (side) return `${capitalized(side[1])} ${modifierWord(side[2], names)}`;
  if (KEY_LABELS[name]) return KEY_LABELS[name];
  return name.split("_").map(capitalized).join(" ");
}

// The caps for a keystroke, each with its words and the name the list gives
// it, for the tooltip: the modifiers, then the key. On a Mac a modifier's
// cap is a mark, such as ⌘. On a PC it is its label, such as Ctrl, set as
// the other caps are.
function caps(keystroke, names) {
  if (!keystroke) return [];
  const known = names || NO_NAMES;
  const pc = pcKeyboard(known);
  if (keystroke.consumer_key_code) {
    const media = listedAs(known.aliases.mediaKeys, keystroke.consumer_key_code);
    const label = (pc ? PC_MEDIA_LABELS : MEDIA_LABELS)[media] || media.replace(/_/g, " ");
    return [{ label, title: media }];
  }
  const held = modifiersShown(keystroke.modifiers, known).map((modifier) => pc
    ? { label: modifierWord(modifier, known), title: modifier }
    : { label: MODIFIER_MARKS[modifier] || modifier, title: modifier, mark: true });
  if (!keystroke.key_code) return held;
  const key = listedAs(known.aliases.keys, keystroke.key_code);
  return [...held, { label: keyLabel(key, known), title: key }];
}

// --- the key name list (D3) -------------------------------------------------

// The groups of the key list, in the order it shows them, each with the
// keys that belong in it. A key goes in the first group, in this order of
// tests, that takes it: the last takes whatever is left. A PC's insert goes
// with the navigation keys, where a Mac keyboard has help.
const KEY_GROUPS = [
  ["Letters", (key) => /^[a-z]$/.test(key)],
  ["Digits", (key) => /^[0-9]$/.test(key)],
  ["Function keys", (key) => /^f\d+$/.test(key)],
  ["Arrows and navigation", (key) => /_arrow$/.test(key) ||
    ["home", "end", "page_up", "page_down", "delete_forward", "help", "insert"].includes(key)],
  ["Keypad", (key) => key.startsWith("keypad_")],
  ["Modifier keys", (key) => MODIFIER_KEYS.includes(key)],
  ["Japanese keyboards", (key) => /^(japanese_|international)/.test(key)],
  ["Controls and symbols", () => true],
];

// The order the list shows the groups in: the keys most actions use first.
const GROUP_ORDER = [
  "Letters", "Digits", "Controls and symbols", "Arrows and navigation",
  "Function keys", "Keypad", "Modifier keys", "Japanese keyboards",
];

// Every key and every media key, grouped for the list: [{label, kind,
// names}], where kind is "key" or "media". Every name in key_names is in it
// once, whatever the recorder can hear.
function keyGroups(names) {
  const known = names || NO_NAMES;
  const grouped = new Map(GROUP_ORDER.map((label) => [label, []]));
  for (const key of known.keys) {
    const [label] = KEY_GROUPS.find(([, takes]) => takes(key));
    grouped.get(label).push(key);
  }
  const groups = GROUP_ORDER
    .map((label) => ({ label, kind: "key", names: grouped.get(label) }))
    .filter((group) => group.names.length > 0);
  if (known.mediaKeys.length > 0) {
    groups.push({ label: "Media keys", kind: "media", names: [...known.mediaKeys] });
  }
  return groups;
}

// A modifier key's label for the key list on a PC: "left_command" reads
// "left_command (Left Win)", so the list names the PC key beside the file's
// name (W2). Empty on a Mac, and for a key with no side label such as
// caps_lock or fn.
function modifierKeyLabel(name, names) {
  if (!pcKeyboard(names)) return name;
  const match = /^(left|right)_(command|option|control|shift)$/.exec(name);
  if (!match) return name;
  const side = match[1] === "left" ? "Left" : "Right";
  const label = names.labels && names.labels[match[2]];
  return label ? `${name} (${side} ${label})` : name;
}

// The keystroke with this key picked from the list. Its modifiers stay, by
// the names the list gives them.
function withKey(keystroke, key, names) {
  const modifiers = keystroke && !keystroke.consumer_key_code
    ? modifiersHeld(keystroke.modifiers, names)
    : [];
  return modifiers.length > 0 ? { key_code: key, modifiers } : { key_code: key };
}

// The keystroke for a media key picked from the list. A media key takes no
// modifiers.
function withMediaKey(media) {
  return { consumer_key_code: media };
}

// The keystroke with one modifier held, or let go. A media key takes none,
// so it stays as it is.
function toggleModifier(keystroke, modifier, names) {
  if (keystroke && keystroke.consumer_key_code) return keystroke;
  const known = names || NO_NAMES;
  const held = new Set(modifiersHeld(keystroke ? keystroke.modifiers : [], known));
  if (held.has(modifier)) held.delete(modifier);
  else held.add(modifier);
  const modifiers = known.modifiers.filter((name) => held.has(name));
  const next = {};
  if (keystroke && keystroke.key_code) next.key_code = keystroke.key_code;
  if (modifiers.length > 0) next.modifiers = modifiers;
  return next;
}

// What goes in the key list's box for a keystroke: "key:4", "media:mute",
// or "" for none.
function pickedValue(keystroke, names) {
  if (!keystroke) return "";
  const known = names || NO_NAMES;
  if (keystroke.consumer_key_code) {
    return `media:${listedAs(known.aliases.mediaKeys, keystroke.consumer_key_code)}`;
  }
  return keystroke.key_code ? `key:${listedAs(known.aliases.keys, keystroke.key_code)}` : "";
}

// --- the page's own recorder --------------------------------------------------
// Without the input permission, the page hears a key itself: its keydown's
// code, which names the key's place, as Karabiner's names do, whatever the
// layout, and the four flags. It never hears a media key, nor a shortcut
// another program takes (B-009b).

// Codes whose name is not a pattern.
const CODE_KEYS = {
  Enter: "return_or_enter", Tab: "tab", Space: "spacebar",
  Backspace: "delete_or_backspace", Escape: "escape",
  Minus: "hyphen", Equal: "equal_sign", BracketLeft: "open_bracket",
  BracketRight: "close_bracket", Backslash: "backslash", Semicolon: "semicolon",
  Quote: "quote", Backquote: "grave_accent_and_tilde", Comma: "comma",
  Period: "period", Slash: "slash", IntlBackslash: "non_us_backslash",
  ArrowUp: "up_arrow", ArrowDown: "down_arrow", ArrowLeft: "left_arrow",
  ArrowRight: "right_arrow", Home: "home", End: "end", PageUp: "page_up",
  PageDown: "page_down", Delete: "delete_forward", Help: "help", Insert: "help",
  NumpadDecimal: "keypad_period", NumpadEnter: "keypad_enter",
  NumpadAdd: "keypad_plus", NumpadSubtract: "keypad_hyphen",
  NumpadMultiply: "keypad_asterisk", NumpadDivide: "keypad_slash",
  NumpadEqual: "keypad_equal_sign", NumpadComma: "keypad_comma",
  NumLock: "keypad_num_lock", NumpadClear: "keypad_num_lock",
  ShiftLeft: "left_shift", ShiftRight: "right_shift",
  ControlLeft: "left_control", ControlRight: "right_control",
  AltLeft: "left_option", AltRight: "right_option",
  MetaLeft: "left_command", MetaRight: "right_command",
  OSLeft: "left_command", OSRight: "right_command",
  CapsLock: "caps_lock", Fn: "fn",
  Lang1: "japanese_kana", Lang2: "japanese_eisuu",
  IntlRo: "international1", IntlYen: "international3",
};

// Codes of media keys, should the page ever hear one.
const CODE_MEDIA_KEYS = {
  AudioVolumeUp: "volume_increment", AudioVolumeDown: "volume_decrement",
  AudioVolumeMute: "mute", MediaPlayPause: "play_or_pause",
  MediaTrackNext: "scan_next_track", MediaTrackPrevious: "scan_previous_track",
  MediaFastForward: "fast_forward", MediaRewind: "rewind", Eject: "eject",
  BrightnessUp: "display_brightness_increment",
  BrightnessDown: "display_brightness_decrement",
};

// Codes of the keys a PC keyboard has beyond a Mac's, by the names Karabiner
// gives them, for a list that offers them. Insert is in the place where a
// Mac keyboard has help.
const PC_CODE_KEYS = {
  Insert: "insert", PrintScreen: "print_screen", ScrollLock: "scroll_lock",
  Pause: "pause", ContextMenu: "application",
};

// Whether the list offers a name. With no list yet, every name is taken.
function offers(list, name) {
  return !list || list.length === 0 || list.includes(name);
}

// The name the list gives the key in a keydown's code, or null. A code whose
// name this system cannot send has none, so the page says so as it hears it.
function keyFromCode(code, names) {
  const keys = names && names.keys;
  let match;
  if ((match = /^Key([A-Z])$/.exec(code))) return match[1].toLowerCase();
  if ((match = /^Digit([0-9])$/.exec(code))) return match[1];
  if ((match = /^Numpad([0-9])$/.exec(code))) return `keypad_${match[1]}`;
  if ((match = /^F([0-9]{1,2})$/.exec(code))) {
    const number = Number(match[1]);
    if (number >= 1 && number <= 20) return `f${number}`;
    return keys && keys.includes(`f${number}`) ? `f${number}` : null;
  }
  const pc = PC_CODE_KEYS[code];
  if (pc && keys && keys.includes(pc)) return pc;
  // A modifier stays one, so the recording waits past it.
  const name = CODE_KEYS[code] || null;
  return name && (MODIFIER_KEYS.includes(name) || offers(keys, name)) ? name : null;
}

// What one keydown the page heard gives: {keystroke}, {modifier: true} for a
// modifier going down, or {unnamed: code} for a key with no name here. The
// modifiers come from the four flags, in the list's order. The page cannot
// tell fn, so a keystroke it hears holds none.
function pageKeystroke(event, names) {
  const media = CODE_MEDIA_KEYS[event.code];
  if (media && offers(names && names.mediaKeys, media)) return { keystroke: withMediaKey(media) };
  const key = keyFromCode(event.code, names);
  if (key && MODIFIER_KEYS.includes(key)) return { modifier: true };
  if (!key) return { unnamed: event.code || event.key || "" };
  const modifiers = [
    ["metaKey", "command"], ["shiftKey", "shift"], ["altKey", "option"], ["ctrlKey", "control"],
  ].filter(([flag]) => event[flag]).map(([, name]) => name);
  return { keystroke: modifiers.length > 0 ? { key_code: key, modifiers } : { key_code: key } };
}

// The note under Record keys (D7), in B-009b's words: with the input
// permission, the tap hears nearly every key; without it, the page hears
// only what reaches the window.
function recordNote(allowed) {
  return allowed
    ? "If a key does not show here, pick it from the list."
    : "This window hears only the keys that reach it. Media keys, and shortcuts that " +
      "macOS or another app owns (such as ⌘⇧4 for screenshots), need input " +
      "access. Allow it, or pick them from the list.";
}

// Try it now on a keystroke counts down first, and says why (D8).
function countdownWords(seconds) {
  return `Sending in ${seconds} s to the app in front. Switch to it now.`;
}

// Under the shortcut picker: how many shortcuts Shortcuts has.
function shortcutsWords(count) {
  if (count === 0) return "No shortcuts found in Shortcuts.app.";
  return `${count} ${count === 1 ? "shortcut" : "shortcuts"} found in Shortcuts.app.`;
}

// --- the editor ---------------------------------------------------------------

// The editor's fields for an action, or for a new one when `action` is
// null. A script opens on the Shell tab, with its file chosen. A web
// request's method shows as it sends; `methodWritten` keeps how the file
// wrote it, so a save that changes nothing writes it back as it was.
function editorState(action) {
  const kind = kindOf(action);
  const request = kind === "web_request" ? action.web_request : null;
  return {
    name: action ? action.name || "" : "",
    signal: action ? action.signal || "" : "",
    tab: kind === "script" ? "shell" : kind || "keystroke",
    shellMode: kind === "script" ? "script" : "shell",
    command: kind === "shell" ? action.shell : "",
    script: kind === "script" ? action.script : "",
    shortcut: kind === "shortcut" ? action.shortcut : "",
    keystroke: kind === "keystroke" ? action.keystroke : null,
    method: request ? methodOf(request) : "GET",
    methodWritten: request && request.method ? request.method : null,
    url: request ? request.url || "" : "",
    headers: request && request.headers ? request.headers.join("\n") : "",
    body: request && typeof request.body === "string" ? request.body : "",
  };
}

// The action the editor's fields make, in the file's shape, for a save or a
// try: the name, the signal, and the kind of the tab in front. The other
// tabs keep what was typed in them, and none of it is sent.
function actionFrom(state) {
  const action = { name: state.name, signal: state.signal };
  if (state.tab === "keystroke") {
    action.keystroke = state.keystroke || {};
  } else if (state.tab === "shell") {
    if (state.shellMode === "script") action.script = state.script;
    else action.shell = state.command;
  } else if (state.tab === "shortcut") {
    action.shortcut = state.shortcut;
  } else if (state.tab === "web_request") {
    action.web_request = requestFrom(state);
  }
  return action;
}

// A web request from the editor's fields. The method is left out when it is
// the one the body implies, unless the file named it: then it stays as the
// file wrote it. Header lines are kept as typed, and blank ones left out.
function requestFrom(state) {
  const request = {};
  const hasBody = state.body !== "";
  const implied = hasBody ? "POST" : "GET";
  if (state.methodWritten && state.methodWritten.trim().toUpperCase() === state.method) {
    request.method = state.methodWritten;
  } else if (state.method !== implied) {
    request.method = state.method;
  }
  request.url = state.url.trim();
  const headers = state.headers.split("\n").filter((line) => line.trim() !== "");
  if (headers.length > 0) request.headers = headers;
  if (hasBody) request.body = state.body;
  return request;
}

// Where the editor shows a problem, by the field the file's rules name:
// name, signal, kind (the tabs), keys, command, script, shortcut, method,
// url, headers, or general, for a problem that is no one field's.
function slotOf(field) {
  if (field === null || field === undefined) return "general";
  if (field === "name" || field === "signal" || field === "kind") return field;
  if (field === "keystroke" || field.startsWith("keystroke.")) return "keys";
  if (field === "shell") return "command";
  if (field === "script" || field === "shortcut") return field;
  if (field === "web_request.method") return "method";
  if (field === "web_request.headers") return "headers";
  if (field === "web_request" || field.startsWith("web_request.")) return "url";
  return "general";
}

// A problem's words as a sentence.
function asSentence(text) {
  const words = String(text || "");
  return words.charAt(0).toUpperCase() + words.slice(1);
}

// Each problem a save or a try came back with, by the place the editor
// shows it: [[slot, words]]. The answer is {errors: [{field, message}]} for
// the file's rules, or a string for anything else.
function problemsBySlot(refused) {
  const errors = refused && Array.isArray(refused.errors)
    ? refused.errors
    : [{ field: null, message: String(refused) }];
  const bySlot = new Map();
  for (const problem of errors) {
    const slot = slotOf(problem.field);
    const words = asSentence(problem.message);
    bySlot.set(slot, bySlot.has(slot) ? `${bySlot.get(slot)} ${words}` : words);
  }
  return [...bySlot];
}

// --- the config file ------------------------------------------------------------

// Whether an error is the config file failing to load.
function isConfigError(text) {
  return /^config error: /.test(String(text || ""));
}

// The loader's reason, without the words that say it is about the config,
// which the note says itself: 'action 3 ("Open Music"): shell is empty'.
function configReason(text) {
  return String(text || "").replace(/^config error: /, "");
}

// What a file that does not load means for the presses. One that failed as
// the app started fires nothing. One that loaded, then broke, leaves the
// band with the actions it held.
function configConsequence(failedAtStart, readBefore) {
  if (failedAtStart) return "No action fires until it is fixed.";
  if (readBefore) return "Until it is fixed, presses do what they did before.";
  return "";
}
