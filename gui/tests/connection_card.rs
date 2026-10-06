//! The words of the connection card and the pill, for each state and reason,
//! as the design frame "State variations — Connection" writes them.
//!
//! The page words the facts the band task sends. `ui/connection.js` holds
//! those words and touches no page, so these tests run it under Node, in New
//! York's time zone, the zone of the times below.
//!
//! The tests are skipped when Node is not installed.

use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

fn node_available() -> bool {
    Command::new("node").arg("--version").output().is_ok()
}

/// What `card` says for each case, where a case is a standing and a battery,
/// or null for a battery not known yet.
fn cards(cases: Value) -> Vec<Value> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/connection.js");
    let program = r#"
        const fs = require("fs");
        const vm = require("vm");
        const page = vm.createContext({});
        vm.runInContext(fs.readFileSync(process.argv[1], "utf8"), page);
        const cases = JSON.parse(process.argv[2]);
        process.stdout.write(JSON.stringify(cases.map(([s, battery]) => page.card(s, battery))));
    "#;
    let out = Command::new("node")
        .env("TZ", "America/New_York")
        .arg("-e")
        .arg(program)
        .arg(script)
        .arg(cases.to_string())
        .output()
        .expect("node runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("node answers with JSON")
}

fn standing(state: &str, reason: &str, since: &str) -> Value {
    json!({ "state": state, "reason": reason, "since": since })
}

fn battery(percent: u8, charging: bool) -> Value {
    json!({ "percent": percent, "charging": charging, "secondsAgo": 12 })
}

/// The pill, the line and the buttons of one card, in one line to compare.
fn read(card: &Value) -> String {
    let buttons: Vec<&str> = card["buttons"]
        .as_array()
        .expect("buttons")
        .iter()
        .map(|b| b.as_str().expect("a button name"))
        .collect();
    format!(
        "{} [{}] | {} [{}, {}] | {}",
        card["pill"].as_str().unwrap(),
        card["pillTone"].as_str().unwrap(),
        card["text"].as_str().unwrap(),
        card["tone"].as_str().unwrap(),
        card["icon"].as_str().unwrap(),
        buttons.join(" ")
    )
}

#[test]
fn the_five_cards_read_as_the_design_draws_them() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let seen = cards(json!([
        [
            standing("connected", "started", "2026-09-24T20:41:07-04:00"),
            battery(74, false)
        ],
        [
            standing("connecting", "you", "2026-09-24T21:10:02-04:00"),
            null
        ],
        [
            standing("disconnected", "you", "2026-09-24T21:06:00-04:00"),
            null
        ],
        [
            standing("retrying", "dropped", "2026-09-24T21:02:13-04:00"),
            null
        ],
        [
            standing("disconnected", "dropped", "2026-09-24T20:58:40-04:00"),
            null
        ],
    ]));
    let read: Vec<String> = seen.iter().map(read).collect();
    assert_eq!(
        read,
        [
            "Connected [good] | 74% · connected since 24 Sep 2026, 20:41 [dim, battery-medium] | disconnect",
            "Connecting [warn] | Reaching out… [warn, loader] | cancel",
            "Not connected [idle] | Disconnected · 24 Sep 2026, 21:06 [dim, circle-slash] | connect forget",
            "Reconnecting [warn] | Connection Dropped · 24 Sep 2026, 21:02 · trying again [warn, refresh-cw] | stop-trying",
            "Connection lost [bad] | Connection Dropped · 24 Sep 2026, 20:58 [bad, triangle-alert] | connect forget",
        ]
    );
    // Connect is Reconnect after a drop, and Connect everywhere else.
    assert_eq!(seen[4]["connect"], "Reconnect");
    assert!(seen[2]["connect"].is_null());
}

#[test]
fn the_three_states_the_design_does_not_draw_follow_its_pattern() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let seen = cards(json!([
        // A first connect failed, and BandBridge tries again.
        [
            standing("retrying", "failed", "2026-09-24T20:52:30-04:00"),
            null
        ],
        // A connect failed, and nothing tries: another band answered.
        [
            standing("disconnected", "failed", "2026-09-24T20:47:05-04:00"),
            null
        ],
        // BandBridge started with no way to reach the band.
        [
            standing("disconnected", "started", "2026-09-24T20:40:50-04:00"),
            null
        ],
    ]));
    let read: Vec<String> = seen.iter().map(read).collect();
    assert_eq!(
        read,
        [
            "Connecting [warn] | Could not connect · 24 Sep 2026, 20:52 · trying again [warn, refresh-cw] | stop-trying",
            "Not connected [idle] | Could not connect · 24 Sep 2026, 20:47 [bad, triangle-alert] | connect forget",
            "Not connected [idle] | Not connected [dim, circle-slash] | connect forget",
        ]
    );
}

/// A connected band shows its battery, with an icon for the level, and says
/// only since when until the band has reported one.
#[test]
fn a_connected_band_shows_its_battery_when_it_has_one() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let up = || standing("connected", "you", "2026-09-24T09:05:00-04:00");
    let seen = cards(json!([
        [up(), null],
        [up(), battery(100, true)],
        [up(), battery(80, false)],
        [up(), battery(30, false)],
        [up(), battery(29, false)],
    ]));
    let lines: Vec<String> = seen
        .iter()
        .map(|card| {
            format!(
                "{} [{}]",
                card["text"].as_str().unwrap(),
                card["icon"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(
        lines,
        [
            "Connected since 24 Sep 2026, 09:05 [circle-check]",
            "100%, charging · connected since 24 Sep 2026, 09:05 [battery-charging]",
            "80% · connected since 24 Sep 2026, 09:05 [battery-full]",
            "30% · connected since 24 Sep 2026, 09:05 [battery-medium]",
            "29% · connected since 24 Sep 2026, 09:05 [battery-low]",
        ]
    );
}

/// Forget is offered while the band is idle, and in no other state, for
/// every reason a state can have.
#[test]
fn forget_is_offered_only_while_the_band_is_idle() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let states = ["connected", "connecting", "retrying", "disconnected"];
    let reasons = ["started", "you", "dropped", "failed"];
    let mut cases = Vec::new();
    for state in states {
        for reason in reasons {
            cases.push(json!([
                standing(state, reason, "2026-09-24T21:00:00-04:00"),
                null
            ]));
        }
    }
    let seen = cards(Value::Array(cases));
    for (i, card) in seen.iter().enumerate() {
        let state = states[i / reasons.len()];
        let reason = reasons[i % reasons.len()];
        let forget = card["buttons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b == "forget");
        assert_eq!(
            forget,
            state == "disconnected",
            "{state} ({reason}): {card}"
        );
    }
}

/// A time the page cannot read is left out, and the rest of the line stays.
#[test]
fn a_time_that_cannot_be_read_is_left_out() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let seen = cards(json!([
        [standing("disconnected", "you", "not a time"), null],
        [standing("retrying", "dropped", ""), null],
        [
            standing("connected", "you", "not a time"),
            battery(74, false)
        ],
    ]));
    let text: Vec<&str> = seen
        .iter()
        .map(|card| card["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        text,
        [
            "Disconnected",
            "Connection Dropped · trying again",
            "74% · connected"
        ]
    );
}
