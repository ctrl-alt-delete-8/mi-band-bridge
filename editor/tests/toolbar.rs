//! Which of the toolbar's controls work, as `ui/toolbar.js` decides it from
//! what the window knows. The page asks it whenever that changes. The script
//! touches no page, so these tests run it under Node.
//!
//! The tests are skipped when Node is not installed.

use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

fn node_available() -> bool {
    Command::new("node").arg("--version").output().is_ok()
}

/// What `controls` answers for each case.
fn controls(cases: &[Value]) -> Vec<Value> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/toolbar.js");
    let program = r#"
        const fs = require("fs");
        const vm = require("vm");
        const page = vm.createContext({});
        vm.runInContext(fs.readFileSync(process.argv[1], "utf8"), page);
        const cases = JSON.parse(process.argv[2]);
        process.stdout.write(JSON.stringify(cases.map((facts) => page.BandToolbar.controls(facts))));
    "#;
    let out = Command::new("node")
        .arg("-e")
        .arg(program)
        .arg(script)
        .arg(Value::from(cases.to_vec()).to_string())
        .output()
        .expect("node runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("node answers with JSON")
}

/// An app open in a folder of three, with no error, nothing running, and
/// BandBridge's band connected: everything works.
fn ready() -> Value {
    json!({
        "toolsMissing": false,
        "folder": true,
        "apps": 3,
        "open": true,
        "errors": 0,
        "run": null,
        "bridge": "connected",
    })
}

/// `ready`, with some facts changed.
fn with(changes: Value) -> Value {
    let mut facts = ready();
    for (key, value) in changes.as_object().expect("an object") {
        facts[key] = value.clone();
    }
    facts
}

/// Install on band, and Install again on the card of an install that
/// stopped, work only while BandBridge says its band is connected. Before
/// its first answer, the window does not know, so they wait. Build does not
/// need BandBridge.
#[test]
fn install_works_only_while_the_band_is_connected() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let states = [
        (json!("connected"), true),
        (json!("not_connected"), false),
        (json!("not_running"), false),
        (Value::Null, false),
    ];
    let cases: Vec<Value> = states
        .iter()
        .map(|(bridge, _)| with(json!({ "bridge": bridge })))
        .collect();
    for ((bridge, on), got) in states.iter().zip(controls(&cases)) {
        assert_eq!(got["install"], json!(on), "Install on band, {bridge}");
        assert_eq!(got["again"], json!(on), "Install again, {bridge}");
        assert_eq!(got["build"], json!(true), "Build, {bridge}");
    }
}

/// The rules from before still hold with the band connected: an app to
/// build and install, no error, and no build or install running. With the
/// build tools missing, nothing works.
#[test]
fn the_rules_from_before_still_hold() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let cases = [
        with(json!({ "errors": 1 })),
        with(json!({ "run": "build" })),
        with(json!({ "run": "install" })),
        with(json!({ "open": false })),
        with(json!({ "toolsMissing": true })),
    ];
    let got = controls(&cases);
    let [error, building, installing, no_app, no_tools] = &got[..] else {
        panic!("{got:?}")
    };

    assert_eq!(
        (&error["build"], &error["install"]),
        (&json!(false), &json!(false))
    );
    // Install again belongs to the install that stopped, which may be
    // another app's, so the open app's error does not stop it.
    assert_eq!(error["again"], json!(true));

    for run in [building, installing] {
        assert_eq!(
            (&run["build"], &run["install"], &run["again"]),
            (&json!(false), &json!(false), &json!(false)),
            "{run}"
        );
    }

    assert_eq!(
        (&no_app["build"], &no_app["install"]),
        (&json!(false), &json!(false))
    );

    for control in ["folder", "apps", "newApp", "build", "install", "again"] {
        assert_eq!(no_tools[control], json!(false), "{control}");
    }
}

/// The folder's controls: Open folder… always, the app picker once the
/// folder holds an app, and a new app once a folder is open.
#[test]
fn the_folder_controls_work_once_they_have_something_to_work_on() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let cases = [
        with(json!({ "folder": false, "apps": 0, "open": false, "bridge": null })),
        with(json!({ "apps": 0, "open": false })),
        ready(),
    ];
    let got = controls(&cases);
    let read = |c: &Value| (c["folder"].clone(), c["apps"].clone(), c["newApp"].clone());
    assert_eq!(read(&got[0]), (json!(true), json!(false), json!(false)));
    assert_eq!(read(&got[1]), (json!(true), json!(false), json!(true)));
    assert_eq!(read(&got[2]), (json!(true), json!(true), json!(true)));
}
