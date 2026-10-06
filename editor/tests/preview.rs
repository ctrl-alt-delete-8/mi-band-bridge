//! Run generated pages with the preview's stand-ins, under Node.
//!
//! The test is skipped when Node is not installed.

use band_editor::generate::generate;
use band_editor::geometry::place;
use band_editor::layout::{lighter, parse_color, Button, Layout};
use std::path::Path;
use std::process::Command;

fn node_available() -> bool {
    Command::new("node").arg("--version").output().is_ok()
}

fn run_check(page: &Path) -> serde_json::Value {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/preview-check.mjs");
    let out = Command::new("node").arg(script).arg(page).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn a_generated_page_draws_its_buttons_and_sends_their_signals() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("slides");
    std::fs::create_dir_all(&dir).unwrap();
    let mut layout = Layout::new("Slides");
    layout.buttons.push(Button::new("bb04", "#1f6feb"));
    layout.buttons.push(Button::new("bb05", "#1f6feb"));
    let made = generate(&dir, &layout).unwrap();

    let result = run_check(&made.project.join("page/index.js"));
    assert_eq!(result["errors"], serde_json::json!([]));
    assert_eq!(result["unknown"], serde_json::json!([]));
    assert_eq!(result["sent"], serde_json::json!(["bb04", "bb05"]));
    let rects = place(&layout);
    for (i, rect) in rects.iter().enumerate() {
        let w = &result["widgets"][i];
        assert_eq!(w["type"], "BUTTON");
        assert_eq!((w["x"].as_i64(), w["y"].as_i64()), (Some(rect.x), Some(rect.y)));
    }
}

/// The band's button gets each label with its line breaks, and every label
/// at the app's one size. "上一页" is three characters of one em each, which
/// fit 87 px buttons at 26 px, so the app takes 26 px, where "Next slide"
/// takes two lines.
#[test]
fn a_page_gives_each_label_its_lines_at_one_size() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("slides");
    std::fs::create_dir_all(&dir).unwrap();
    let mut layout = Layout::new("Slides");
    layout.columns = 2;
    for (signal, label) in [("bb04", "Next slide"), ("bb05", "上一页")] {
        let mut button = Button::new(signal, "#1f6feb");
        button.label = label.to_string();
        layout.buttons.push(button);
    }
    let made = generate(&dir, &layout).unwrap();

    let result = run_check(&made.project.join("page/index.js"));
    assert_eq!(result["errors"], serde_json::json!([]));
    assert_eq!(result["widgets"][0]["text"], "Next\nslide");
    assert_eq!(result["widgets"][1]["text"], "上一页");
    assert_eq!(result["widgets"][0]["text_size"], 26);
    assert_eq!(result["widgets"][1]["text_size"], 26);
}

#[test]
fn a_page_that_throws_reports_the_error() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let page = dir.path().join("index.js");
    std::fs::write(
        &page,
        "var m = __$$hmAppManager$$__.currentApp.current;\nm.module = DeviceRuntimeCore.Page({ onInit: function () { undefinedCall(); } });\n",
    )
    .unwrap();
    let result = run_check(&page);
    assert!(result["errors"][0].as_str().unwrap().contains("undefinedCall"));
}

/// The form works out the automatic pressed color itself, so the swatch can
/// follow the button's color with no round trip. That copy of the rule must
/// keep agreeing with the one the band draws.
#[test]
fn the_form_and_the_generator_agree_on_the_automatic_pressed_color() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let cases = ["#000000", "#ffffff", "#1f6feb", "#010203", "#7f7f7f"];
    let colors = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/colors.js");
    let list = cases.iter().map(|c| format!("{c:?}")).collect::<Vec<_>>().join(", ");
    let out = Command::new("node")
        .arg("-e")
        .arg(format!(
            "const c = require({colors:?}); console.log(JSON.stringify([{list}].map(c.autoPressColor)));"
        ))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let from_form: Vec<String> = serde_json::from_slice(&out.stdout).unwrap();

    let from_generator: Vec<String> = cases
        .iter()
        .map(|c| {
            let (r, g, b) = lighter(parse_color(c).unwrap());
            format!("#{r:02x}{g:02x}{b:02x}")
        })
        .collect();
    assert_eq!(from_form, from_generator);
}

#[test]
fn an_unknown_widget_is_named() {
    if !node_available() {
        eprintln!("skipped: node is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let page = dir.path().join("index.js");
    std::fs::write(
        &page,
        "var m = __$$hmAppManager$$__.currentApp.current;\nm.module = DeviceRuntimeCore.Page({ onInit: function () { hmUI.createWidget(hmUI.widget.ARC, {}); } });\n",
    )
    .unwrap();
    let result = run_check(&page);
    assert_eq!(result["unknown"], serde_json::json!(["ARC"]));
}
