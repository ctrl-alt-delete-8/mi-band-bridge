//! The actions section's words and data, as the design frame "BandBridge
//! Window — Light" draws its section "What each button does", and the frame
//! "State variations — Action kinds" draws the editor's four tabs.
//!
//! `ui/actions.js` holds them and touches no page, so these tests run it
//! under Node. Where the page and the library must agree, the tests ask the
//! library itself: the key names come from `bandbridge::keys::names()`, and
//! every action the editor makes is read by `Action::from_editor`, as a save
//! reads it.
//!
//! The tests are skipped when Node is not installed.

use bandbridge::config::Action;
use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

fn node_available() -> bool {
    Command::new("node").arg("--version").output().is_ok()
}

/// The names the window's key list gets, as `key_names` answers them.
fn names() -> Value {
    serde_json::to_value(bandbridge::keys::names()).unwrap()
}

/// Run `program` under Node, with `ui/actions.js` loaded, `input` set, and
/// `names` set to what `key_names` answers, and give back the value of its
/// last expression.
fn run(program: &str, input: Value) -> Value {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/actions.js");
    let host = r#"
        const fs = require("fs");
        const vm = require("vm");
        const page = vm.createContext({});
        vm.runInContext(fs.readFileSync(process.argv[1], "utf8"), page);
        page.input = JSON.parse(process.argv[2]);
        page.names = JSON.parse(process.argv[3]);
        process.stdout.write(JSON.stringify(vm.runInContext(process.argv[4], page)));
    "#;
    let out = Command::new("node")
        .arg("-e")
        .arg(host)
        .arg(script)
        .arg(input.to_string())
        .arg(names().to_string())
        .arg(program)
        .output()
        .expect("node runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("node answers with JSON")
}

/// The design's four actions, in the file's shape, as `actions_list` gives
/// them, and a script and a media key besides.
fn the_designs_actions() -> Value {
    json!([
        {"name": "Screenshot a region", "signal": "bb01",
         "keystroke": {"key_code": "4", "modifiers": ["left_command", "left_shift"]}},
        {"name": "Start a focus block", "signal": "bb02", "shortcut": "Start Focus"},
        {"name": "Open Music", "signal": "bb03", "shell": "open -a Music"},
        {"name": "Turn on the desk lights", "signal": "bb04",
         "web_request": {"method": "POST", "url": "http://lights.local/webhook",
                         "headers": ["Authorization: Bearer …"], "body": "{\"on\": true}"}},
        {"name": "Toggle the desk lamp", "signal": "bb06", "script": "~/scripts/toggle-desk-lamp.sh"},
        {"name": "Play", "signal": "bb05", "keystroke": {"consumer_key_code": "play_or_pause"}},
    ])
}

/// The list's rows, as `actions_list` gives the actions: each with its kind.
fn listed(actions: &Value) -> Value {
    Value::Array(
        actions
            .as_array()
            .unwrap()
            .iter()
            .map(|action| {
                let kind = ["keystroke", "shell", "script", "shortcut", "web_request"]
                    .into_iter()
                    .find(|kind| action.get(*kind).is_some())
                    .unwrap();
                json!({"kind": kind, "action": action})
            })
            .collect(),
    )
}

#[test]
fn each_row_reads_as_the_design_draws_it() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let rows = run(
        r#"input.map((listed) => {
            const row = listRow(listed, names);
            return `${row.signal} ${row.name} | ${row.kind} | ${row.value} | ${row.mono ? "mono" : "ui"} | ${row.title}`;
        })"#,
        listed(&the_designs_actions()),
    );
    // On Windows the modifiers read by their labels, in the Windows order,
    // Ctrl, Alt, Shift, Win (W2).
    #[cfg(target_os = "macos")]
    let screenshot = "bb01 Screenshot a region | Keystroke | command shift 4 | mono | command shift 4";
    #[cfg(windows)]
    let screenshot = "bb01 Screenshot a region | Keystroke | Shift Win 4 | mono | Shift Win 4";
    assert_eq!(
        rows,
        json!([
            screenshot,
            "bb02 Start a focus block | Shortcut | Start Focus | ui | Start Focus",
            "bb03 Open Music | Shell | open -a Music | mono | open -a Music",
            "bb04 Turn on the desk lights | Web request | POST lights.local/webhook | mono | POST http://lights.local/webhook",
            "bb06 Toggle the desk lamp | Script | ~/scripts/toggle-desk-lamp.sh | mono | ~/scripts/toggle-desk-lamp.sh",
            "bb05 Play | Keystroke | play_or_pause | mono | play_or_pause",
        ])
    );
    // A request's method is the one it sends: GET with no body, POST with one.
    let methods = run(
        r#"[
            listRow({kind: "web_request", action: {name: "a", signal: "bb07", web_request: {url: "https://example.com/hook"}}}, names).value,
            listRow({kind: "web_request", action: {name: "b", signal: "bb08", web_request: {url: "http://x.local/", body: ""}}}, names).value,
            listRow({kind: "web_request", action: {name: "c", signal: "bb09", web_request: {method: "put", url: "http://x.local/"}}}, names).value,
            countWords(4), countWords(1), countWords(0),
        ]"#,
        Value::Null,
    );
    assert_eq!(
        methods,
        json!([
            "GET example.com/hook",
            "POST x.local/",
            "PUT x.local/",
            "4 set",
            "1 set",
            "none set"
        ])
    );
}

/// The list offers every key and every media key this system sends, each
/// once, in its groups, whatever the recorder can hear (D3): on the Mac 116
/// keys and 13 media keys. Windows sends 117 keys and 6 media keys: no fn,
/// help or Japanese keys, and the keys only a PC has, insert with the
/// navigation keys and f21 to f24 with the function keys.
#[test]
fn the_key_list_offers_every_key_and_media_key_once() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let groups = run("keyGroups(names)", Value::Null);
    let labels: Vec<&str> = groups
        .as_array()
        .unwrap()
        .iter()
        .map(|group| group["label"].as_str().unwrap())
        .collect();
    #[cfg(target_os = "macos")]
    let (group_labels, key_count, media_count, function_key_count, last_navigation_key) = (
        vec![
            "Letters",
            "Digits",
            "Controls and symbols",
            "Arrows and navigation",
            "Function keys",
            "Keypad",
            "Modifier keys",
            "Japanese keyboards",
            "Media keys",
        ],
        116,
        13,
        20,
        "help",
    );
    #[cfg(windows)]
    let (group_labels, key_count, media_count, function_key_count, last_navigation_key) = (
        vec![
            "Letters",
            "Digits",
            "Controls and symbols",
            "Arrows and navigation",
            "Function keys",
            "Keypad",
            "Modifier keys",
            "Media keys",
        ],
        117,
        6,
        24,
        "insert",
    );
    assert_eq!(labels, group_labels);
    let offered = |kind: &str| -> Vec<String> {
        groups
            .as_array()
            .unwrap()
            .iter()
            .filter(|group| group["kind"] == kind)
            .flat_map(|group| group["names"].as_array().unwrap().iter())
            .map(|name| name.as_str().unwrap().to_string())
            .collect()
    };
    let names = bandbridge::keys::names();
    let mut keys = offered("key");
    let mut expected: Vec<String> = names.keys.iter().map(|k| k.to_string()).collect();
    keys.sort();
    expected.sort();
    assert_eq!(keys, expected, "every key once");
    assert_eq!(keys.len(), key_count);
    let media = offered("media");
    assert_eq!(
        media, names.media_keys,
        "every media key, in the list's order"
    );
    assert_eq!(media.len(), media_count);

    let function_keys = &groups[4]["names"];
    assert_eq!(function_keys[0], "f1");
    assert_eq!(function_keys.as_array().unwrap().len(), function_key_count);
    assert_eq!(
        groups[3]["names"],
        json!([
            "up_arrow",
            "down_arrow",
            "left_arrow",
            "right_arrow",
            "home",
            "end",
            "page_up",
            "page_down",
            "delete_forward",
            last_navigation_key
        ])
    );
    // The keys only a PC has, beside return and tab.
    #[cfg(windows)]
    {
        let controls = groups[2]["names"].as_array().unwrap();
        for key in ["print_screen", "scroll_lock", "pause", "application"] {
            assert!(controls.contains(&json!(key)), "{key} in {controls:?}");
        }
    }
}

/// When the recorder cannot hear, the page hears a key itself: its keydown's
/// code names its place, as the list does, and its four flags the
/// modifiers. Every name it gives is one the list offers, so on Windows it
/// names the keys only a PC has, and a key Windows cannot send has none.
#[test]
fn the_page_names_a_key_by_its_place_as_the_list_does() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let heard = run(
        r#"[
            pageKeystroke({code: "Digit4", metaKey: true, shiftKey: true}, names),
            pageKeystroke({code: "KeyK"}, names),
            pageKeystroke({code: "KeyK", ctrlKey: true, altKey: true, shiftKey: true, metaKey: true}, names),
            pageKeystroke({code: "F5"}, names),
            pageKeystroke({code: "ArrowUp"}, names),
            pageKeystroke({code: "Enter"}, names),
            pageKeystroke({code: "Numpad7"}, names),
            pageKeystroke({code: "MetaLeft", metaKey: true}, names),
            pageKeystroke({code: "ShiftRight", shiftKey: true}, names),
            pageKeystroke({code: "CapsLock"}, names),
            pageKeystroke({code: "AudioVolumeUp", shiftKey: true}, names),
            pageKeystroke({code: "", key: "Unidentified"}, names),
        ]"#,
        Value::Null,
    );
    assert_eq!(
        heard,
        json!([
            {"keystroke": {"key_code": "4", "modifiers": ["command", "shift"]}},
            {"keystroke": {"key_code": "k"}},
            {"keystroke": {"key_code": "k", "modifiers": ["command", "shift", "option", "control"]}},
            {"keystroke": {"key_code": "f5"}},
            {"keystroke": {"key_code": "up_arrow"}},
            {"keystroke": {"key_code": "return_or_enter"}},
            {"keystroke": {"key_code": "keypad_7"}},
            {"modifier": true},
            {"modifier": true},
            {"modifier": true},
            {"keystroke": {"consumer_key_code": "volume_increment"}},
            {"unnamed": "Unidentified"},
        ])
    );

    // The keys in the place of insert, and past f20, and the media keys only
    // a Mac sends, by what each system's list offers.
    let each_system = run(
        r#"["Insert", "F21", "F24", "PrintScreen", "ContextMenu", "Help", "Fn",
             "NumpadEqual", "MediaFastForward"]
            .map((code) => pageKeystroke({code}, names))"#,
        Value::Null,
    );
    #[cfg(target_os = "macos")]
    let expected = json!([
        {"keystroke": {"key_code": "help"}},
        {"unnamed": "F21"},
        {"unnamed": "F24"},
        {"unnamed": "PrintScreen"},
        {"unnamed": "ContextMenu"},
        {"keystroke": {"key_code": "help"}},
        {"modifier": true},
        {"keystroke": {"key_code": "keypad_equal_sign"}},
        {"keystroke": {"consumer_key_code": "fast_forward"}},
    ]);
    #[cfg(windows)]
    let expected = json!([
        {"keystroke": {"key_code": "insert"}},
        {"keystroke": {"key_code": "f21"}},
        {"keystroke": {"key_code": "f24"}},
        {"keystroke": {"key_code": "print_screen"}},
        {"keystroke": {"key_code": "application"}},
        {"unnamed": "Help"},
        {"modifier": true},
        {"unnamed": "NumpadEqual"},
        {"unnamed": "MediaFastForward"},
    ]);
    assert_eq!(each_system, expected);

    // Every name the page gives is one the list offers, or a modifier it
    // waits past. On the Mac, every code the page knows has a name.
    let codes = run(
        r#"(() => {
            const codes = [...Object.keys(CODE_KEYS), ...Object.keys(PC_CODE_KEYS)];
            for (let c = 65; c <= 90; c += 1) codes.push(`Key${String.fromCharCode(c)}`);
            for (let d = 0; d <= 9; d += 1) codes.push(`Digit${d}`, `Numpad${d}`);
            for (let f = 1; f <= 24; f += 1) codes.push(`F${f}`);
            return {
              keys: codes.map((code) => [code, keyFromCode(code, names)]),
              media: Object.keys(CODE_MEDIA_KEYS).map((code) => [code, pageKeystroke({code}, names)]),
              waits: MODIFIER_KEYS,
            };
        })()"#,
        Value::Null,
    );
    let names = bandbridge::keys::names();
    let waits = codes["waits"].as_array().unwrap();
    // The codes of keys the other system's keyboard has, which have no name
    // here.
    #[cfg(target_os = "macos")]
    let other_systems = [
        "PrintScreen", "ScrollLock", "Pause", "ContextMenu", "F21", "F22", "F23", "F24",
    ];
    #[cfg(windows)]
    let other_systems = [
        "Help", "NumpadEqual", "NumpadComma", "Lang1", "Lang2", "IntlRo", "IntlYen",
    ];
    for pair in codes["keys"].as_array().unwrap() {
        let code = pair[0].as_str().unwrap();
        let Some(name) = pair[1].as_str() else {
            assert!(other_systems.contains(&code), "{code} has no name");
            continue;
        };
        assert!(
            names.keys.contains(&name) || waits.contains(&json!(name)),
            "{} gives {name}",
            pair[0]
        );
    }
    // Every media code the page knows must give a name the list offers, or
    // be one this system does not send. A code that fell out of the list
    // gives no name and is not refused, so it fails here, as it did before.
    #[cfg(target_os = "macos")]
    let refused_media: [&str; 0] = [];
    #[cfg(windows)]
    let refused_media = [
        "MediaFastForward",
        "MediaRewind",
        "Eject",
        "BrightnessUp",
        "BrightnessDown",
    ];
    for pair in codes["media"].as_array().unwrap() {
        let code = pair[0].as_str().unwrap();
        let Some(name) = pair[1]["keystroke"]["consumer_key_code"].as_str() else {
            assert!(refused_media.contains(&code), "{code} gives no media key");
            continue;
        };
        assert!(names.media_keys.contains(&name), "{code} gives {name}");
    }
}

/// The caps show the modifiers as marks, then the key, whatever spelling
/// the file holds, with the list's name on each tooltip.
#[cfg(target_os = "macos")]
#[test]
fn the_caps_read_as_keys() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let caps = run(
        r#"[
            {key_code: "4", modifiers: ["left_command", "left_shift"]},
            {key_code: "k", modifiers: ["ctrl", "alt", "right_shift", "cmd", "fn"]},
            {key_code: "return"},
            {key_code: "up_arrow", modifiers: ["option"]},
            {key_code: "f5"},
            {key_code: "keypad_1"},
            {key_code: "grave_accent_and_tilde"},
            {consumer_key_code: "volume_increment"},
            {consumer_key_code: "fastforward"},
            {modifiers: ["command"]},
        ].map((keystroke) => caps(keystroke, names).map((cap) => `${cap.label}=${cap.title}`).join(" "))"#,
        Value::Null,
    );
    assert_eq!(
        caps,
        json!([
            "⌘=command ⇧=shift 4=4",
            "⌘=command ⇧=shift ⌥=option ⌃=control fn=fn K=k",
            "return=return_or_enter",
            "⌥=option ↑=up_arrow",
            "F5=f5",
            "keypad 1=keypad_1",
            "`=grave_accent_and_tilde",
            "volume up=volume_increment",
            "fast forward=fast_forward",
            "⌘=command",
        ])
    );
}

/// On Windows the caps read as a PC keyboard prints its keys: the modifiers
/// by their labels, Ctrl, Alt, Shift and Win (W2), set as the other caps
/// are, then the key, with the list's name on each tooltip. The keys only a
/// PC has are named as Windows names them.
#[cfg(windows)]
#[test]
fn the_caps_read_as_keys() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let caps = run(
        r#"[
            {key_code: "4", modifiers: ["left_command", "left_shift"]},
            {key_code: "k", modifiers: ["ctrl", "alt", "right_shift", "cmd"]},
            {key_code: "return"},
            {key_code: "up_arrow", modifiers: ["option"]},
            {key_code: "f24"},
            {key_code: "keypad_1"},
            {key_code: "grave_accent_and_tilde"},
            {key_code: "delete_or_backspace"},
            {key_code: "delete_forward"},
            {key_code: "print_screen", modifiers: ["control"]},
            {key_code: "application"},
            {key_code: "insert"},
            {key_code: "scroll_lock"},
            {key_code: "right_option"},
            {consumer_key_code: "volume_increment"},
            {consumer_key_code: "play_or_pause"},
            {modifiers: ["command"]},
        ].map((keystroke) => caps(keystroke, names)
            .map((cap) => `${cap.label}=${cap.title}${cap.mark ? " (mark)" : ""}`).join(" "))"#,
        Value::Null,
    );
    assert_eq!(
        caps,
        json!([
            "Shift=shift Win=command 4=4",
            "Ctrl=control Alt=option Shift=shift Win=command K=k",
            "Enter=return_or_enter",
            "Alt=option ↑=up_arrow",
            "F24=f24",
            "Keypad 1=keypad_1",
            "`=grave_accent_and_tilde",
            "Backspace=delete_or_backspace",
            "Delete=delete_forward",
            "Ctrl=control Print Screen=print_screen",
            "Menu=application",
            "Insert=insert",
            "Scroll Lock=scroll_lock",
            "Right Alt=right_option",
            "Volume Up=volume_increment",
            "Play/Pause=play_or_pause",
            "Win=command",
        ])
    );
}

/// A key picked from the list keeps the modifiers held, a media key drops
/// them, and a modifier toggles on and off, in the list's order.
#[test]
fn picking_from_the_list_keeps_or_drops_the_modifiers() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let steps = run(
        r#"(() => {
            const steps = [];
            let keys = null;
            keys = toggleModifier(keys, "shift", names); steps.push(keys);
            keys = withKey(keys, "4", names); steps.push(keys);
            keys = toggleModifier(keys, "command", names); steps.push(keys);
            steps.push(pickedValue(keys, names));
            keys = toggleModifier(keys, "shift", names); steps.push(keys);
            keys = withKey({key_code: "k", modifiers: ["left_cmd"]}, "f18", names); steps.push(keys);
            keys = withMediaKey("mute"); steps.push(keys);
            steps.push(toggleModifier(keys, "command", names));
            steps.push(pickedValue(keys, names));
            steps.push(pickedValue({key_code: "return"}, names));
            steps.push(pickedValue(null, names));
            return steps;
        })()"#,
        Value::Null,
    );
    assert_eq!(
        steps,
        json!([
            {"modifiers": ["shift"]},
            {"key_code": "4", "modifiers": ["shift"]},
            {"key_code": "4", "modifiers": ["command", "shift"]},
            "key:4",
            {"key_code": "4", "modifiers": ["command"]},
            {"key_code": "f18", "modifiers": ["command"]},
            {"consumer_key_code": "mute"},
            {"consumer_key_code": "mute"},
            "media:mute",
            "key:return_or_enter",
            "",
        ])
    );
}

/// Every kind goes through the editor and back unchanged, and the library
/// reads what the editor sends back as the same action: a save of an action
/// nobody changed writes the file as it was.
#[test]
fn every_kind_goes_through_the_editor_and_back_unchanged() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let mut actions = the_designs_actions();
    for extra in [
        json!({"name": "Hook", "signal": "bb07", "web_request": {"url": "https://example.com/hook"}}),
        json!({"name": "Post", "signal": "bb08", "web_request": {"url": "http://x.local/", "body": "on"}}),
        json!({"name": "Get with a body", "signal": "bb09",
               "web_request": {"method": "get", "url": "http://x.local/", "body": "on"}}),
        json!({"name": "Put", "signal": "bb0a",
               "web_request": {"method": "PUT", "url": "http://x.local/", "headers": ["A: 1", "B: 2"]}}),
        json!({"name": "Page up", "signal": "bb0b", "keystroke": {"key_code": "page_up"}}),
    ] {
        actions.as_array_mut().unwrap().push(extra);
    }
    let back = run(
        "input.map((action) => actionFrom(editorState(action)))",
        actions.clone(),
    );
    assert_eq!(back, actions);
    for action in actions.as_array().unwrap() {
        // Windows has no Shortcut kind (W5): the editor does not offer it,
        // and a save of one is refused in the loader's words.
        if cfg!(windows) && action.get("shortcut").is_some() {
            let refused = serde_json::to_value(Action::from_editor(action).unwrap_err()).unwrap();
            assert!(
                refused.to_string().contains("Shortcuts are a Mac feature"),
                "{refused}"
            );
            continue;
        }
        let read =
            Action::from_editor(action).unwrap_or_else(|refused| panic!("{action}: {refused}"));
        assert_eq!(&serde_json::to_value(&read).unwrap(), action);
    }
}

/// Each tab of the editor makes its own kind, and keeps what was typed on
/// the others without sending it. A script is on the Shell tab: whichever of
/// the command and the file was set last is the one sent.
#[test]
fn each_tab_makes_its_own_kind() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let made = run(
        r#"(() => {
            const state = editorState(null);
            const made = [state.tab, actionFrom(state)];
            state.name = "Lamp"; state.signal = "bb06";
            state.command = "open -a Music";
            state.tab = "shell"; made.push(actionFrom(state));
            state.script = "~/scripts/lamp.sh"; state.shellMode = "script"; made.push(actionFrom(state));
            state.shortcut = "Start Focus"; state.tab = "shortcut"; made.push(actionFrom(state));
            state.url = " http://lights.local/webhook "; state.method = "POST";
            state.headers = "Authorization: Bearer …\n\n"; state.body = "";
            state.tab = "web_request"; made.push(actionFrom(state));
            state.method = "GET"; state.body = "{}"; made.push(actionFrom(state));
            state.keystroke = {key_code: "4", modifiers: ["command", "shift"]};
            state.tab = "keystroke"; made.push(actionFrom(state));
            return made;
        })()"#,
        Value::Null,
    );
    assert_eq!(
        made,
        json!([
            "keystroke",
            {"name": "", "signal": "", "keystroke": {}},
            {"name": "Lamp", "signal": "bb06", "shell": "open -a Music"},
            {"name": "Lamp", "signal": "bb06", "script": "~/scripts/lamp.sh"},
            {"name": "Lamp", "signal": "bb06", "shortcut": "Start Focus"},
            {"name": "Lamp", "signal": "bb06", "web_request": {
                "method": "POST", "url": "http://lights.local/webhook",
                "headers": ["Authorization: Bearer …"]}},
            {"name": "Lamp", "signal": "bb06", "web_request": {
                "method": "GET", "url": "http://lights.local/webhook",
                "headers": ["Authorization: Bearer …"], "body": "{}"}},
            {"name": "Lamp", "signal": "bb06", "keystroke": {"key_code": "4", "modifiers": ["command", "shift"]}},
        ])
    );
}

/// Every problem a save comes back with goes beside the field it names.
/// The problems here are the library's own, for a new action saved with
/// nothing filled in, and for each rule a kind can break.
#[test]
fn each_problem_goes_beside_its_field() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let refusals: Vec<Value> = [
        json!({"name": "", "signal": "", "keystroke": {}}),
        json!({"name": "Lamp", "signal": "cc01", "shell": " "}),
        json!({"name": "Lamp", "signal": "bb06", "script": ""}),
        json!({"name": "Lamp", "signal": "bb06", "shortcut": ""}),
        json!({"name": "Lamp", "signal": "bb06", "keystroke": {"key_code": "f99"}}),
        json!({"name": "Lamp", "signal": "bb06", "keystroke": {"key_code": "k", "modifiers": ["meta"]}}),
        json!({"name": "Lamp", "signal": "bb06", "keystroke": {"consumer_key_code": "play"}}),
        json!({"name": "Lamp", "signal": "bb06", "web_request": {"url": "lights.local"}}),
        json!({"name": "Lamp", "signal": "bb06", "web_request": {"url": "http://x/", "method": "FETCH"}}),
        json!({"name": "Lamp", "signal": "bb06", "web_request": {"url": "http://x/", "headers": ["no colon"]}}),
        json!({"name": "Lamp", "signal": "bb06"}),
    ]
    .iter()
    .map(|action| serde_json::to_value(Action::from_editor(action).unwrap_err()).unwrap())
    .collect();
    let slots = run(
        "input.map((refused) => problemsBySlot(refused).map(([slot]) => slot).join(' '))",
        Value::Array(refusals),
    );
    assert_eq!(
        slots,
        json!([
            "name signal keys",
            "signal command",
            "script",
            "shortcut",
            "keys",
            "keys",
            "keys",
            "url",
            "method",
            "headers",
            "kind",
        ])
    );
    let every_field = run(
        r#"[null, "name", "signal", "kind", "keystroke", "keystroke.key_code",
            "keystroke.consumer_key_code", "keystroke.modifiers", "shell", "script",
            "shortcut", "web_request", "web_request.url", "web_request.method",
            "web_request.headers", "something new"].map(slotOf)"#,
        Value::Null,
    );
    assert_eq!(
        every_field,
        json!([
            "general", "name", "signal", "kind", "keys", "keys", "keys", "keys", "command",
            "script", "shortcut", "url", "url", "method", "headers", "general"
        ])
    );
    // A problem's words become a sentence. A string is one problem that is
    // no field's.
    let words = run(
        r#"[
            problemsBySlot({errors: [{field: "shell", message: "shell is empty"}]}),
            problemsBySlot("config error: cannot write the file"),
        ]"#,
        Value::Null,
    );
    assert_eq!(
        words,
        json!([
            [["command", "Shell is empty"]],
            [["general", "Config error: cannot write the file"]],
        ])
    );
}

/// The notes say what B-009b and the plan's decisions say, in their words.
#[test]
fn the_notes_read_as_decided() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let notes = run(
        r#"[
            recordNote(true), recordNote(false),
            countdownWords(3), countdownWords(1),
            shortcutsWords(12), shortcutsWords(1), shortcutsWords(0),
        ]"#,
        Value::Null,
    );
    assert_eq!(
        notes,
        json!([
            "If a key does not show here, pick it from the list.",
            "This window hears only the keys that reach it. Media keys, and shortcuts that macOS or another app owns (such as ⌘⇧4 for screenshots), need input access. Allow it, or pick them from the list.",
            "Sending in 3 s to the app in front. Switch to it now.",
            "Sending in 1 s to the app in front. Switch to it now.",
            "12 shortcuts found in Shortcuts.app.",
            "1 shortcut found in Shortcuts.app.",
            "No shortcuts found in Shortcuts.app.",
        ])
    );
}

/// A file that does not load: the loader's reason, which names the action
/// and the field, and what it means for the presses.
#[test]
fn a_file_that_does_not_load_says_why_and_what_it_means() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let broken = bandbridge::config::Config::parse(
        "[[actions]]\nname = \"Open Music\"\nsignal = \"bb03\"\nshell = \"\"\n",
    )
    .unwrap_err()
    .to_string();
    let read = run(
        r#"[
            isConfigError(input), configReason(input),
            isConfigError("the band task is gone"), configReason("the band task is gone"),
            configConsequence(true, false), configConsequence(false, true),
            configConsequence(false, false),
        ]"#,
        json!(broken),
    );
    assert_eq!(read[0], true, "{broken}");
    let reason = read[1].as_str().unwrap();
    assert!(
        reason.contains("Open Music") && reason.contains("shell is empty"),
        "{reason}"
    );
    assert!(!reason.starts_with("config error"), "{reason}");
    assert_eq!(
        read.as_array().unwrap()[2..],
        [
            json!(false),
            json!("the band task is gone"),
            json!("No action fires until it is fixed."),
            json!("Until it is fixed, presses do what they did before."),
            json!(""),
        ]
    );
}
