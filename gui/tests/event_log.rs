//! The event log's rows, for each kind of record and each result, as the
//! design frame "BandBridge Window — Light" draws them in its section "Events
//! from the device", and which records the log keeps. A row has a result chip
//! only where the result says more than that the action worked: a web
//! request's HTTP status, or a failure.
//!
//! `ui/events.js` holds the rows' words and the count of what the log shows,
//! and touches no page, so these tests run it under Node, in New York's time
//! zone, the zone of the times below. The records are the ones the band task
//! sends (see `Record` in `src/event.rs`).
//!
//! The tests are skipped when Node is not installed.

use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

fn node_available() -> bool {
    Command::new("node").arg("--version").output().is_ok()
}

/// Helpers the programs below share: `read` puts a row in one line to
/// compare, and `play` runs records through the log as the page does, and
/// reads what it then shows, newest first.
const PRELUDE: &str = r#"
    function read(row) {
      if (!row) return null;
      const head = `${row.time} [${row.tag}] `;
      if (row.kind === "app") return head + `${row.change} ${row.app}`;
      if (!row.action) {
        const extra = row.running || row.result ? " AND MORE" : "";
        return head + `${row.signal} (${row.unset})${extra}`;
      }
      const end = row.running ? "running"
        : row.result ? `${row.result.words} (${row.result.tone})` : "no chip";
      return head + `${row.signal} → ${row.action} | ${end}`;
    }
    function play(ledger, rows, records) {
      for (const record of records) {
        const row = eventRow(record);
        if (!row) continue;
        const step = ledger.take(record.id);
        if (step.act === "drop") continue;
        rows.set(record.id, row);
        for (const id of step.trim) rows.delete(id);
      }
      return ledger.shown().map((id) => read(rows.get(id)));
    }
"#;

/// Run `program` under Node, with `ui/events.js` and the prelude loaded and
/// `input` set, and give back the value of its last expression.
fn run(program: &str, input: Value) -> Value {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/events.js");
    let host = r#"
        const fs = require("fs");
        const vm = require("vm");
        const page = vm.createContext({});
        vm.runInContext(fs.readFileSync(process.argv[1], "utf8"), page);
        vm.runInContext(process.argv[2], page);
        page.input = JSON.parse(process.argv[3]);
        process.stdout.write(JSON.stringify(vm.runInContext(process.argv[4], page)));
    "#;
    let out = Command::new("node")
        .env("TZ", "America/New_York")
        .arg("-e")
        .arg(host)
        .arg(script)
        .arg(PRELUDE)
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

/// Play the records through a log that holds `limit` rows, clearing it
/// wherever a record is the string "clear", and read what it shows.
fn shown_after(records: Value, limit: usize) -> Vec<String> {
    let seen = run(
        r#"(() => {
            const ledger = eventLedger(input.limit);
            const rows = new Map();
            for (const record of input.records) {
              if (record === "clear") {
                ledger.clear();
                rows.clear();
              } else {
                play(ledger, rows, [record]);
              }
            }
            return play(ledger, rows, []);
        })()"#,
        json!({ "records": records, "limit": limit }),
    );
    serde_json::from_value(seen).expect("a list of rows")
}

fn press(id: u64, at: &str, signal: &str, action: Option<&str>, result: Value) -> Value {
    json!({
        "kind": "press", "id": id, "at": format!("2026-09-24T{at}-04:00"),
        "signal": signal, "action": action, "result": result,
    })
}

fn app(id: u64, at: &str, change: &str, name: &str) -> Value {
    json!({
        "kind": "app", "id": id, "at": format!("2026-09-24T{at}-04:00"),
        "change": change, "app": name,
    })
}

/// The records of the design's six rows, as the band task sends them: a
/// press whose action takes time comes first with no result, then again with
/// it. A keystroke comes once, with its result.
fn the_designs_records() -> Value {
    let done = json!({"type": "done", "ok": true});
    let music = |result| press(2, "21:02:12", "bb03", Some("Open Music"), result);
    let focus = |result| press(4, "21:03:20", "bb02", Some("Start a focus block"), result);
    let region = |result| press(5, "21:04:07", "bb01", Some("Screenshot a region"), result);
    let lights = |result| {
        press(
            6,
            "21:04:11",
            "bb04",
            Some("Turn on the desk lights"),
            result,
        )
    };
    json!([
        app(1, "20:41:07", "removed", "BandArrows"),
        music(Value::Null),
        music(json!({"type": "exit", "ok": true, "code": 0})),
        app(3, "21:02:58", "installed", "testbandapp"),
        focus(Value::Null),
        focus(done.clone()),
        region(done),
        lights(Value::Null),
        lights(json!({"type": "status", "ok": true, "code": 204})),
    ])
}

#[test]
fn the_designs_six_rows_read_as_it_draws_them_newest_first() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    assert_eq!(
        shown_after(the_designs_records(), 500),
        [
            "21:04:11 [press] bb04 → Turn on the desk lights | 204 (good)",
            "21:04:07 [press] bb01 → Screenshot a region | no chip",
            "21:03:20 [press] bb02 → Start a focus block | no chip",
            "21:02:58 [app] installed testbandapp",
            "21:02:12 [press] bb03 → Open Music | no chip",
            "20:41:07 [app] removed BandArrows",
        ]
    );
}

/// Each result the band task can send, from the table in B-008a's report,
/// and the chip it gets: the words a terminal prints, the tone from `ok`,
/// and the words in full on the tooltip.
#[test]
fn every_result_has_its_words_tone_and_mark() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let not_allowed = "macOS does not let this program post key events, so the keystroke went \
                       nowhere. Allow it in System Settings, Privacy & Security, Device Control \
                       and Data Access.";
    let no_shortcut = "exit 1: The shortcut “Start Focus” could not be found.";
    let cases = json!([
        {"type": "status", "ok": true, "code": 204},
        {"type": "status", "ok": false, "code": 503},
        {"type": "error", "ok": false, "error": "failed: Connection refused (os error 61)"},
        {"type": "done", "ok": true},
        {"type": "error", "ok": false, "error": not_allowed},
        {"type": "error", "ok": false, "error": no_shortcut},
        {"type": "exit", "ok": true, "code": 0},
        {"type": "exit", "ok": false, "code": 4, "error": "the lamp is away"},
        {"type": "exit", "ok": false, "code": 127},
        {"type": "error", "ok": false, "error": "stopped by a signal"},
        {"type": "error", "ok": false, "error": "cannot run it: No such file or directory (os error 2)"},
        {"type": "error", "ok": false, "error": "gave up after 10 seconds"},
        {"type": "error", "ok": false},
    ]);
    let chips = run("input.map(resultChip)", cases);
    let read: Vec<String> = chips
        .as_array()
        .expect("chips")
        .iter()
        .map(|chip| {
            format!(
                "{} [{}, {}] {}",
                chip["words"].as_str().unwrap(),
                chip["tone"].as_str().unwrap(),
                chip["mark"].as_str().unwrap(),
                chip["title"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            "204 [good, check] HTTP 204".to_string(),
            "503 [bad, x] HTTP 503".to_string(),
            "failed: Connection refused (os error 61) [bad, x] failed: Connection refused (os error 61)".to_string(),
            "done [good, check] done".to_string(),
            format!("{not_allowed} [bad, x] {not_allowed}"),
            format!("{no_shortcut} [bad, x] {no_shortcut}"),
            "exit 0 [good, check] exit 0".to_string(),
            "exit 4: the lamp is away [bad, x] exit 4: the lamp is away".to_string(),
            "exit 127 [bad, x] exit 127".to_string(),
            "stopped by a signal [bad, x] stopped by a signal".to_string(),
            "cannot run it: No such file or directory (os error 2) [bad, x] cannot run it: No such file or directory (os error 2)".to_string(),
            "gave up after 10 seconds [bad, x] gave up after 10 seconds".to_string(),
            "failed [bad, x] failed".to_string(),
        ]
    );
}

/// A chip only where the result tells more than that the action worked, as
/// the design draws it: a web request's status always has one, green or red,
/// and every failure has a red one. A plain success, done or exit 0, has
/// none, and no spinner either.
#[test]
fn only_a_status_or_a_failure_has_a_chip() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let results = json!([
        {"type": "done", "ok": true},
        {"type": "exit", "ok": true, "code": 0},
        {"type": "status", "ok": true, "code": 204},
        {"type": "status", "ok": true, "code": 302},
        {"type": "status", "ok": false, "code": 503},
        {"type": "exit", "ok": false, "code": 4, "error": "the lamp is away"},
        {"type": "exit", "ok": false, "code": 127},
        {"type": "error", "ok": false, "error": "gave up after 10 seconds"},
    ]);
    let records: Vec<Value> = results
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(n, result)| {
            press(
                n as u64 + 1,
                "21:04:11",
                "bb04",
                Some("Lamp"),
                result.clone(),
            )
        })
        .collect();
    let seen = run(
        "input.map((record) => read(eventRow(record)))",
        Value::Array(records),
    );
    assert_eq!(
        seen,
        json!([
            "21:04:11 [press] bb04 → Lamp | no chip",
            "21:04:11 [press] bb04 → Lamp | no chip",
            "21:04:11 [press] bb04 → Lamp | 204 (good)",
            "21:04:11 [press] bb04 → Lamp | 302 (good)",
            "21:04:11 [press] bb04 → Lamp | 503 (bad)",
            "21:04:11 [press] bb04 → Lamp | exit 4: the lamp is away (bad)",
            "21:04:11 [press] bb04 → Lamp | exit 127 (bad)",
            "21:04:11 [press] bb04 → Lamp | gave up after 10 seconds (bad)",
        ])
    );
    let done = run(
        "eventRow(input)",
        press(
            9,
            "21:04:11",
            "bb01",
            Some("Page up"),
            json!({"type": "done", "ok": true}),
        ),
    );
    assert_eq!(done["running"], false, "a success is over, not running");
    assert!(done["result"].is_null(), "{done}");
}

/// A press whose action takes time shows its row at once, running, with no
/// chip. The chip fills in when the result comes, in the same row.
#[test]
fn a_running_press_shows_at_once_and_its_chip_fills_in_later() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let lights = |result| {
        press(
            14,
            "21:04:11",
            "bb04",
            Some("Turn on the desk lights"),
            result,
        )
    };
    let running = lights(Value::Null);
    let row = run("eventRow(input)", running.clone());
    assert_eq!(row["running"], true);
    assert!(row["result"].is_null(), "{row}");

    assert_eq!(
        shown_after(json!([running.clone()]), 500),
        ["21:04:11 [press] bb04 → Turn on the desk lights | running"]
    );
    let ended = lights(json!({"type": "error", "ok": false, "error": "gave up after 10 seconds"}));
    assert_eq!(
        shown_after(json!([running, ended]), 500),
        ["21:04:11 [press] bb04 → Turn on the desk lights | gave up after 10 seconds (bad)"]
    );
}

/// A press that no action answers shows its signal and says no action is
/// set for it, with no arrow, no chip, and nothing running.
#[test]
fn a_press_no_action_answers_says_so_with_no_arrow_and_no_chip() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let unset = press(3, "20:52:01", "bb09", None, Value::Null);
    let row = run("eventRow(input)", unset.clone());
    assert!(row["action"].is_null(), "{row}");
    assert_eq!(row["unset"], "no action set");
    assert_eq!(row["running"], false);
    assert!(row["result"].is_null(), "{row}");
    assert_eq!(
        shown_after(json!([unset]), 500),
        ["20:52:01 [press] bb09 (no action set)"]
    );
}

/// Clear takes every row away. A result that comes after it, for a row that
/// is gone, is dropped, not shown as a new row. A press that comes after it
/// is a new row.
#[test]
fn after_clear_a_late_result_is_dropped_and_a_new_press_is_shown() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let lamp = |result| press(7, "21:05:00", "bb06", Some("Toggle the desk lamp"), result);
    let region = press(
        8,
        "21:05:02",
        "bb01",
        Some("Screenshot a region"),
        json!({"type": "done", "ok": true}),
    );
    let lights = press(
        9,
        "21:05:09",
        "bb04",
        Some("Turn on the desk lights"),
        Value::Null,
    );
    let records = json!([
        lamp(Value::Null),
        region,
        "clear",
        lamp(json!({"type": "exit", "ok": true, "code": 0})),
        lights,
    ]);
    assert_eq!(
        shown_after(records, 500),
        ["21:05:09 [press] bb04 → Turn on the desk lights | running"]
    );

    // Clear with nothing shown, then the first records ever.
    let records = json!(["clear", app(1, "20:41:07", "removed", "BandArrows")]);
    assert_eq!(
        shown_after(records, 500),
        ["20:41:07 [app] removed BandArrows"]
    );
}

/// One row per record, newest first: a new record goes above every older
/// row, and a record that comes again replaces its own row.
#[test]
fn each_record_keeps_one_row_in_order_of_its_id() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let steps = run(
        r#"(() => {
            const ledger = eventLedger(500);
            return [5, 6, 6, 3, 4, 7].map((id) => [id, ledger.take(id), ledger.shown()]);
        })()"#,
        Value::Null,
    );
    assert_eq!(
        steps,
        json!([
            [5, {"act": "add", "before": null, "trim": []}, [5]],
            [6, {"act": "add", "before": 5, "trim": []}, [6, 5]],
            [6, {"act": "replace", "before": null, "trim": []}, [6, 5]],
            [3, {"act": "add", "before": null, "trim": []}, [6, 5, 3]],
            [4, {"act": "add", "before": 3, "trim": []}, [6, 5, 4, 3]],
            [7, {"act": "add", "before": 6, "trim": []}, [7, 6, 5, 4, 3]],
        ])
    );
}

/// A full log lets its oldest row go, and that row is gone for good: a
/// result that comes for it later is dropped. The rows still shown take
/// their results.
#[test]
fn the_oldest_row_leaves_a_full_log_and_stays_gone() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let steps = run(
        r#"(() => {
            const ledger = eventLedger(3);
            return [1, 2, 3, 4, 1, 2].map((id) => [id, ledger.take(id).act, ledger.shown()]);
        })()"#,
        Value::Null,
    );
    assert_eq!(
        steps,
        json!([
            [1, "add", [1]],
            [2, "add", [2, 1]],
            [3, "add", [3, 2, 1]],
            [4, "add", [4, 3, 2]],
            [1, "drop", [4, 3, 2]],
            [2, "replace", [4, 3, 2]],
        ])
    );
    let fourth = run(
        "(() => { const l = eventLedger(3); [1, 2, 3].forEach((id) => l.take(id)); return l.take(4); })()",
        Value::Null,
    );
    assert_eq!(fourth, json!({"act": "add", "before": 3, "trim": [1]}));
}

/// The rows of the `events` set in the preview: every kind, every result,
/// and a press still running on top.
#[test]
fn every_kind_of_row_reads_in_one_line() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let error = |why: &str| json!({"type": "error", "ok": false, "error": why});
    let focus = press(
        4,
        "20:58:13",
        "bb02",
        Some("Start a focus block"),
        error("exit 1: The shortcut “Start Focus” could not be found."),
    );
    let lamp_away = json!({"type": "exit", "ok": false, "code": 4, "error": "the lamp is away"});
    let lamp = press(
        5,
        "20:59:02",
        "bb06",
        Some("Toggle the desk lamp"),
        lamp_away,
    );
    let lights = |id, at, result| press(id, at, "bb04", Some("Turn on the desk lights"), result);
    let records = json!([
        app(1, "20:41:07", "removed", "BandArrows"),
        app(2, "20:44:30", "installed", "slides.bin"),
        press(3, "20:52:01", "bb09", None, Value::Null),
        focus,
        lamp,
        lights(
            6,
            "21:00:45",
            error("failed: Connection refused (os error 61)")
        ),
        lights(
            7,
            "21:01:30",
            json!({"type": "status", "ok": false, "code": 503})
        ),
        lights(8, "21:04:30", Value::Null),
    ]);
    assert_eq!(
        shown_after(records, 500),
        [
            "21:04:30 [press] bb04 → Turn on the desk lights | running",
            "21:01:30 [press] bb04 → Turn on the desk lights | 503 (bad)",
            "21:00:45 [press] bb04 → Turn on the desk lights | failed: Connection refused (os error 61) (bad)",
            "20:59:02 [press] bb06 → Toggle the desk lamp | exit 4: the lamp is away (bad)",
            "20:58:13 [press] bb02 → Start a focus block | exit 1: The shortcut “Start Focus” could not be found. (bad)",
            "20:52:01 [press] bb09 (no action set)",
            "20:44:30 [app] installed slides.bin",
            "20:41:07 [app] removed BandArrows",
        ]
    );
}

/// The time is local, on a 24-hour clock, read from the record's own
/// offset. A time that cannot be read shows as dashes. A kind this page
/// does not know gets no row.
#[test]
fn the_time_is_local_and_an_unknown_kind_has_no_row() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let seen = run(
        r#"[
            clock("2026-09-24T21:04:11-04:00"),
            clock("2026-09-25T01:04:11Z"),
            clock("2026-09-24T09:05:07-04:00"),
            clock("not a time"),
            eventRow({kind: "notice", id: 9, at: "2026-09-24T21:04:11-04:00"}),
        ]"#,
        Value::Null,
    );
    assert_eq!(
        seen,
        json!(["21:04:11", "21:04:11", "09:05:07", "--:--:--", null])
    );
}
