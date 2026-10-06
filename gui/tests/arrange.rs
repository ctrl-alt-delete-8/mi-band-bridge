//! The apps and the band's screen lists, as the design frame "BandBridge
//! Window — Light" draws its sections "Install or remove an app" and
//! "Arrange what the device shows".
//!
//! `ui/arrange.js` holds their words, and the rules for a move in a list,
//! and touches no page, so these tests run it under Node. Where the page and
//! the library must agree, the tests ask the library itself: the band's
//! names for its screens come from `bandbridge::menu_names`, the entries in
//! the shape `bandbridge::screens::Entry` gives them, and the icons from the
//! page's own sprite.
//!
//! The tests are skipped when Node is not installed.

use bandbridge::screens::Entry;
use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

fn node_available() -> bool {
    Command::new("node").arg("--version").output().is_ok()
}

/// Run `program` under Node, with `ui/arrange.js` loaded and `input` set,
/// and give back the value of its last expression.
fn run(program: &str, input: Value) -> Value {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/arrange.js");
    let host = r#"
        const fs = require("fs");
        const vm = require("vm");
        const page = vm.createContext({});
        vm.runInContext(fs.readFileSync(process.argv[1], "utf8"), page);
        page.input = JSON.parse(process.argv[2]);
        process.stdout.write(JSON.stringify(vm.runInContext(process.argv[3], page)));
    "#;
    let out = Command::new("node")
        .arg("-e")
        .arg(host)
        .arg(script)
        .arg(input.to_string())
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

/// An entry as the band task gives it to the window, in the library's own
/// shape.
fn entry(id: &str, name: &str, is_app: bool, installed_here: bool, shown: bool) -> Value {
    serde_json::to_value(Entry {
        id: id.to_string(),
        name: name.to_string(),
        is_app,
        installed_here,
        shown,
        position: 0,
    })
    .unwrap()
}

/// The design's app menu: six shown, two hidden.
fn the_designs_menu() -> Value {
    json!([
        entry("58E537B5", "BandBridge", true, true, true),
        entry("10BD37D4", "testbandapp", true, true, true),
        entry("00000003", "workout", false, false, true),
        entry("00000002", "hr", false, false, true),
        entry("00000023", "sleep", false, false, true),
        entry("00000004", "weather", false, false, true),
        entry("0000001C", "stress", false, false, false),
        entry("00000024", "spo2", false, false, false),
    ])
}

/// The icons the page's sprite holds.
fn sprite() -> Vec<String> {
    let page = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/index.html"))
        .expect("the page");
    page.split("<symbol id=\"i-")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap().to_string())
        .collect()
}

/// Every screen the band names, in the app menu and in the shortcuts, has
/// words of its own and an icon the page can draw.
#[test]
fn every_screen_the_band_names_has_words_and_an_icon() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let screens: Vec<Value> = bandbridge::menu_names::MENU
        .iter()
        .chain(bandbridge::menu_names::SHORTCUTS)
        .map(|(id, name)| entry(id, name, false, false, true))
        .collect();
    let named = run(
        r#"input.map((entry) => [entry.name, entry.name in SCREENS, screenLabel(entry), screenIcon(entry)])"#,
        Value::Array(screens),
    );
    let sprite = sprite();
    for row in named.as_array().unwrap() {
        let (name, known, label, icon) = (&row[0], &row[1], &row[2], &row[3]);
        assert_eq!(known, &json!(true), "{name} has no words");
        assert!(
            label
                .as_str()
                .is_some_and(|l| !l.is_empty() && !l.contains('_')),
            "{name}: {label}"
        );
        assert!(
            sprite.iter().any(|s| Some(s.as_str()) == icon.as_str()),
            "{name}: the sprite has no {icon}"
        );
    }
    // Every icon the table names, and the ones for apps and the unknown,
    // is in the sprite.
    let icons = run(
        r#"[...new Set([...Object.values(SCREENS).map(([, icon]) => icon), OTHER_SCREEN_ICON,
             screenIcon({is_app: true, id: BANDBRIDGE_APP}), screenIcon({is_app: true, id: "10BD37D4"})])]"#,
        Value::Null,
    );
    for icon in icons.as_array().unwrap() {
        assert!(
            sprite.iter().any(|s| Some(s.as_str()) == icon.as_str()),
            "the sprite has no {icon}"
        );
    }
    // A screen a later band adds reads as its own name.
    assert_eq!(
        run(
            r#"[screenLabel(input), screenIcon(input)]"#,
            entry("0010FFFF", "new_thing", false, false, true)
        ),
        json!(["New thing", "layout-grid"])
    );
}

/// An app goes by its name, and by its id when this Mac knows none. The
/// custom tag marks an app installed from here, and only that. The
/// BandBridge band app wears the window's own mark, as the design draws it.
#[test]
fn an_app_goes_by_its_name_and_the_custom_tag_marks_the_ones_installed_here() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    // The page marks the BandBridge band app by its id, so that id must be
    // the one in the band app's own manifest.
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../band-app/app.json");
    let (id, name) = bandbridge::app_names::from_manifest(&manifest)
        .expect("the BandBridge band app has a manifest");
    assert_eq!(name, "BandBridge");
    let ours = id.as_str();
    assert_eq!(run("BANDBRIDGE_APP", Value::Null), json!(ours));

    let rows = run(
        r#"input.map((entry) => `${screenLabel(entry)} <${screenIcon(entry)}>${isCustom(entry) ? " custom" : ""} | ${screenTitle(entry)}`)"#,
        json!([
            entry(ours, "BandBridge", true, true, true),
            entry("10BD37D4", "testbandapp", true, true, true),
            entry("0001A2B3", "0001A2B3", true, false, true),
            entry("00000002", "hr", false, false, true),
        ]),
    );
    assert_eq!(
        rows,
        json!([
            "BandBridge <watch> custom | BandBridge (58E537B5)",
            "testbandapp <box> custom | testbandapp (10BD37D4)",
            "0001A2B3 <box> | 0001A2B3 (0001A2B3)",
            "Heart rate <heart> | Heart rate (00000002)",
        ])
    );
}

/// The columns read as the design writes them: the shown screens in the
/// band's order, the hidden ones by their words, and the count in the head.
#[test]
fn a_column_reads_as_the_design_writes_it() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let read = run(
        r#"({
            shown: shownEntries(input).map(screenLabel),
            hidden: hiddenEntries(input).map(screenLabel),
            count: listCounts(input),
            none: listCounts([]),
            lists: Object.keys(LISTS),
            titles: Object.values(LISTS).map((list) => list.title),
        })"#,
        the_designs_menu(),
    );
    assert_eq!(
        read,
        json!({
            "shown": ["BandBridge", "testbandapp", "Workout", "Heart rate", "Sleep", "Weather"],
            "hidden": ["SpO2", "Stress"],
            "count": "6 shown · 2 hidden",
            "none": "0 shown · 0 hidden",
            "lists": ["menu", "shortcuts"],
            "titles": ["App menu", "Shortcuts"],
        })
    );
}

/// A move gives the band the shown screens in their new order. The
/// design's move takes testbandapp to after Heart rate. A screen dropped on
/// the hidden group leaves the list, and one from it joins at its place. A
/// move that changes nothing sends nothing.
#[test]
fn a_move_gives_the_order_the_band_is_sent() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let moves = run(
        r#"[
            moveTo("menu", input, "10BD37D4", 3),
            moveTo("menu", input, "10BD37D4", 0),
            moveTo("menu", input, "00000004", "hidden"),
            moveTo("menu", input, "00000024", 2),
            moveTo("menu", input, "00000024", 99),
            moveTo("menu", input, "10BD37D4", 1),
            moveTo("menu", input, "0000001C", "hidden"),
            moveTo("menu", input, "DEADBEEF", 0),
            moveTo("control", input, "10BD37D4", 0),
        ]"#,
        the_designs_menu(),
    );
    assert_eq!(
        moves,
        json!([
            {"shown": ["58E537B5", "00000003", "00000002", "10BD37D4", "00000023", "00000004"]},
            {"shown": ["10BD37D4", "58E537B5", "00000003", "00000002", "00000023", "00000004"]},
            {"shown": ["58E537B5", "10BD37D4", "00000003", "00000002", "00000023"]},
            {"shown": ["58E537B5", "10BD37D4", "00000024", "00000003", "00000002", "00000023", "00000004"]},
            {"shown": ["58E537B5", "10BD37D4", "00000003", "00000002", "00000023", "00000004", "00000024"]},
            null,
            null,
            null,
            null,
        ])
    );
    // The list the window draws while the band takes it.
    let drawn = run(
        r#"(() => {
            const after = arranged(input, moveTo("menu", input, "10BD37D4", 3).shown);
            return {
                shown: shownEntries(after).map((e) => `${e.id}@${e.position}`),
                hidden: hiddenEntries(after).map((e) => e.id),
                asAsked: showsAsAsked(after, moveTo("menu", input, "10BD37D4", 3).shown),
                before: showsAsAsked(input, moveTo("menu", input, "10BD37D4", 3).shown),
            };
        })()"#,
        the_designs_menu(),
    );
    assert_eq!(
        drawn,
        json!({
            "shown": ["58E537B5@0", "00000003@1", "00000002@2", "10BD37D4@3", "00000023@4", "00000004@5"],
            "hidden": ["00000024", "0000001C"],
            "asAsked": true,
            "before": false,
        })
    );
}

/// What the band does not take is refused before anything is sent, with
/// words that say why: it keeps Settings in its app menu, and shows at most
/// ten shortcuts. Moving within ten shortcuts is fine.
#[test]
fn a_move_the_band_would_not_take_is_refused_with_its_reason() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let mut menu = the_designs_menu().as_array().unwrap().clone();
    menu.push(entry("00000013", "settings", false, false, true));
    let ten: Vec<Value> = (1..=10)
        .map(|n| entry(&format!("{n:08X}"), "hr", false, false, true))
        .chain([entry("00000012", "breathing", false, false, false)])
        .collect();
    let refused = run(
        r#"[
            moveTo("menu", input.menu, "00000013", "hidden"),
            moveTo("menu", input.menu, "00000013", 0),
            moveTo("shortcuts", input.ten, "00000012", 0),
            moveTo("shortcuts", input.ten, "00000001", 9),
            moveTo("shortcuts", input.ten, "00000001", "hidden").shown.length,
        ]"#,
        json!({"menu": menu, "ten": ten}),
    );
    assert_eq!(
        refused[0],
        json!({"refused": "The device keeps Settings in its app menu."})
    );
    assert_eq!(
        refused[1]["shown"][0],
        json!("00000013"),
        "Settings moves within the menu"
    );
    assert_eq!(
        refused[2],
        json!({"refused": "The device shows at most 10 shortcuts. Hide one first."})
    );
    assert_eq!(refused[3]["shown"][9], json!("00000001"));
    assert_eq!(refused[4], json!(9));
}

/// With the keyboard, a screen steps through each place among the others,
/// then the hidden group, and a person hears where it would land.
#[test]
fn the_keyboard_steps_through_each_place_then_the_hidden_group() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let steps = run(
        r#"({
            shownRow: placesFor(input, "10BD37D4"),
            hiddenRow: placesFor(input, "00000024"),
            at: [placeOf(input, "10BD37D4"), placeOf(input, "00000024")],
            words: [
                placeWords("menu", input, "10BD37D4", 3),
                placeWords("menu", input, "00000024", 2),
                placeWords("shortcuts", input, "10BD37D4", "hidden"),
            ],
        })"#,
        the_designs_menu(),
    );
    assert_eq!(
        steps,
        json!({
            "shownRow": [0, 1, 2, 3, 4, 5, "hidden"],
            "hiddenRow": [0, 1, 2, 3, 4, 5, 6, "hidden"],
            "at": [1, "hidden"],
            "words": [
                "testbandapp: place 4 of 6 in the app menu.",
                "SpO2: place 3 of 7 in the app menu.",
                "testbandapp: hidden.",
            ],
        })
    );
}

/// An app's row, the question before a removal, and what the install says,
/// as the window writes them. The band task's reasons lose the words that
/// say which part failed, which the window says itself.
#[test]
fn the_apps_words_read_as_the_window_writes_them() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let path = "/Users/you/Projects/band-apps/slides/dist/slides.bin";
    let words = run(
        r#"[
            appRow(input.app),
            appRow({id: "2D38", version: "v1.0"}),
            removeQuestion(input.app),
            appsEmptyWords(),
            installWords("running", input.path),
            installWords("done", input.path),
            installWords("failed", input.path, "install failed: the band is not connected. Connect first."),
            packageName("C:\\apps\\slides.bin"),
            reasonOf("apps: the band has no app 10BD37D4. It lists: 58E537B5"),
            reasonOf("display list: the band did not report its menu"),
            reasonOf("refusing to send 900000 bytes"),
            awayWords("away", "its apps"),
            awayWords("coming", "this list"),
        ]"#,
        json!({
            "app": {"id": "10BD37D4", "number": 280836052, "version": "1.0.2", "name": "testbandapp",
                    "is_watchface": false, "in_use": false},
            "path": path,
        }),
    );
    assert_eq!(
        words,
        json!([
            {"name": "testbandapp", "id": "10BD37D4", "version": "v1.0.2"},
            {"name": "2D38", "id": "2D38", "version": "v1.0"},
            "Remove “testbandapp” from the device? To put it back, install its .bin again.",
            "No apps installed from here yet.",
            {"tone": "busy", "words": "Installing slides.bin. The device stays connected until the app is on, then connects again."},
            {"tone": "good", "words": "Installed slides.bin."},
            {"tone": "bad", "words": "slides.bin was not installed: the band is not connected. Connect first."},
            "slides.bin",
            "the band has no app 10BD37D4. It lists: 58E537B5",
            "the band did not report its menu",
            "refusing to send 900000 bytes",
            "Connect the device to see its apps.",
            "Waiting for the device to connect, to read this list.",
        ])
    );
}
