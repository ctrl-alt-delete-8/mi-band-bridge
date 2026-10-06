//! The file a template app is made from: `layout.json`.
//!
//! This file is the whole app. Everything else is made from it on every build.

use crate::geometry::DEFAULT_BOTTOM_STRIP;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// The newest `layout.json` format this editor reads and writes.
pub const FORMAT: u32 = 1;

/// The color a new app and a new button start with.
pub const DEFAULT_COLOR: &str = "#1f6feb";

/// The shape of every button in an app.
///
/// One choice for the whole app, not one per button: an app whose buttons have
/// different corners does not read as one app, and a radius in pixels is a knob
/// with no right answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Corners {
    /// Half circles at the ends, like the band's own apps.
    #[default]
    Pill,
    /// A rectangle with a small round corner.
    Rounded,
    /// A plain rectangle.
    Square,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub format: u32,
    pub name: String,
    pub app_id: u32,
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub color: String,
    pub columns: u32,
    #[serde(default)]
    pub corners: Corners,
    #[serde(default = "default_bottom_strip")]
    pub bottom_strip: u32,
    /// Labels may go below 25 px, the smallest size easy to read on the band,
    /// down to 10 px. Off unless the person allows it. Written only when on,
    /// so a layout from before keeps its bytes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub small_text: bool,
    #[serde(default)]
    pub buttons: Vec<Button>,
    /// Keys of warnings the user dismissed. A key names the value it warned
    /// about, so a change to that value brings the warning back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dismissed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Button {
    #[serde(default)]
    pub label: String,
    pub signal: String,
    pub color: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub press_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub span: bool,
}

fn default_bottom_strip() -> u32 {
    DEFAULT_BOTTOM_STRIP
}

impl Layout {
    /// A new app with no buttons and a fresh app id.
    pub fn new(name: &str) -> Layout {
        Layout {
            format: FORMAT,
            name: name.to_string(),
            app_id: new_app_id(),
            version: 1,
            icon: None,
            color: DEFAULT_COLOR.to_string(),
            columns: 1,
            corners: Corners::default(),
            bottom_strip: DEFAULT_BOTTOM_STRIP,
            small_text: false,
            buttons: Vec::new(),
            dismissed: Vec::new(),
        }
    }
}

impl Button {
    pub fn new(signal: &str, color: &str) -> Button {
        Button {
            label: String::new(),
            signal: signal.to_string(),
            color: color.to_string(),
            press_color: None,
            image: None,
            span: false,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("cannot read or write {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("{path} is not a valid layout: {source}")]
    Json {
        path: String,
        source: serde_json::Error,
    },
    #[error("{path} uses layout format {found}. This editor reads format {max} or older. Update the editor.")]
    TooNew { path: String, found: u32, max: u32 },
}

pub fn load(path: &Path) -> Result<Layout, LayoutError> {
    let shown = path.display().to_string();
    let text = std::fs::read_to_string(path).map_err(|source| LayoutError::Io {
        path: shown.clone(),
        source,
    })?;
    let layout: Layout = serde_json::from_str(&text).map_err(|source| LayoutError::Json {
        path: shown.clone(),
        source,
    })?;
    if layout.format > FORMAT {
        return Err(LayoutError::TooNew {
            path: shown,
            found: layout.format,
            max: FORMAT,
        });
    }
    Ok(layout)
}

pub fn save(path: &Path, layout: &Layout) -> Result<(), LayoutError> {
    let mut text = serde_json::to_string_pretty(layout).map_err(|source| LayoutError::Json {
        path: path.display().to_string(),
        source,
    })?;
    text.push('\n');
    std::fs::write(path, text).map_err(|source| LayoutError::Io {
        path: path.display().to_string(),
        source,
    })
}

/// A random app id in the range zmake itself uses for a new project.
pub fn new_app_id() -> u32 {
    rand::random_range(0x0000_FFFF..=0x7FFF_FFFF)
}

/// Hex text to bytes. One to eight bytes, upper or lower case.
pub fn parse_signal(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.is_empty() || text.len() % 2 != 0 || text.len() > 16 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// The first byte of every signal this editor makes.
///
/// The band's own platform messages share the endpoint an app sends on, and
/// they are told apart by the first byte: 0x01 is JavaScript, 0x02 is app
/// management, 0x03 is a screenshot. One prefix for every app here keeps well
/// clear of them, and leaves 252 signals, which is far more than a wrist
/// needs.
pub const SIGNAL_PREFIX: u8 = 0xbb;

/// The signal as stored and matched: lower case hex.
pub fn normalize_signal(text: &str) -> Option<String> {
    let bytes = parse_signal(text)?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// `#rrggbb` to red, green, and blue.
pub fn parse_color(text: &str) -> Option<(u8, u8, u8)> {
    let hex = text.trim().strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let part = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    Some((part(0)?, part(2)?, part(4)?))
}

/// The default pressed color: each channel half way to white.
pub fn lighter((r, g, b): (u8, u8, u8)) -> (u8, u8, u8) {
    let half = |c: u8| c + (255 - c) / 2;
    (half(r), half(g), half(b))
}

/// A name made safe for a folder: lower case letters, digits, and hyphens.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "app".to_string() } else { out }
}

/// An image path from `layout.json`, resolved inside the app folder.
///
/// None for a path that leaves the folder, because a hand-edited layout could
/// otherwise make the editor read any file on the computer.
pub fn image_path(app_dir: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    let leaves = rel.components().any(|c| {
        matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_))
    });
    if rel.is_absolute() || leaves {
        return None;
    }
    Some(app_dir.join(rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Layout {
        let mut layout = Layout::new("Slides");
        layout.buttons.push(Button::new("bb04", "#1f6feb"));
        layout
    }

    #[test]
    fn a_layout_survives_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layout.json");
        let layout = sample();
        save(&path, &layout).unwrap();
        assert_eq!(load(&path).unwrap(), layout);
    }

    #[test]
    fn the_file_uses_camel_case_keys() {
        let text = serde_json::to_string(&sample()).unwrap();
        assert!(text.contains("\"appId\""), "{text}");
        assert!(text.contains("\"bottomStrip\""), "{text}");
    }

    #[test]
    fn missing_optional_fields_take_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layout.json");
        std::fs::write(
            &path,
            r##"{"format":1,"name":"A","appId":70000,"version":1,"color":"#000000","columns":1}"##,
        )
        .unwrap();
        let layout = load(&path).unwrap();
        assert_eq!(layout.bottom_strip, 70);
        assert!(layout.buttons.is_empty());
        assert!(layout.dismissed.is_empty());
        assert_eq!(layout.icon, None);
        assert!(!layout.small_text);
    }

    /// Smaller text is off unless the person allows it. The file names it
    /// only when it is on, so a layout from before keeps its bytes.
    #[test]
    fn smaller_text_is_written_only_when_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layout.json");
        let mut layout = sample();
        save(&path, &layout).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        assert!(!before.contains("smallText"), "{before}");

        layout.small_text = true;
        save(&path, &layout).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"smallText\": true"), "{text}");
        assert_eq!(load(&path).unwrap(), layout);

        layout.small_text = false;
        save(&path, &layout).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn a_broken_file_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layout.json");
        std::fs::write(&path, "{").unwrap();
        let message = load(&path).unwrap_err().to_string();
        assert!(message.contains("layout.json"), "{message}");
        assert!(message.contains("not a valid layout"), "{message}");
    }

    #[test]
    fn a_newer_format_is_refused_with_a_clear_message() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layout.json");
        std::fs::write(
            &path,
            r##"{"format":99,"name":"A","appId":70000,"version":1,"color":"#000000","columns":1}"##,
        )
        .unwrap();
        let message = load(&path).unwrap_err().to_string();
        assert!(message.contains("99"), "{message}");
    }

    #[test]
    fn a_new_app_id_is_in_the_range_zmake_uses() {
        for _ in 0..200 {
            let id = new_app_id();
            assert!((0x0000_FFFF..=0x7FFF_FFFF).contains(&id));
        }
    }

    #[test]
    fn signals_are_hex_bytes() {
        assert_eq!(parse_signal("bb04"), Some(vec![0xbb, 0x04]));
        assert_eq!(normalize_signal(" BB04 "), Some("bb04".to_string()));
        assert_eq!(parse_signal("bb0"), None, "odd number of digits");
        assert_eq!(parse_signal("zz"), None, "not hex");
        assert_eq!(parse_signal(""), None, "empty");
        assert_eq!(parse_signal("0102030405060708"), Some(vec![1, 2, 3, 4, 5, 6, 7, 8]));
        assert_eq!(parse_signal("010203040506070809"), None, "more than 8 bytes");
    }

    #[test]
    fn colors_are_hash_and_six_hex_digits() {
        assert_eq!(parse_color("#1f6feb"), Some((0x1f, 0x6f, 0xeb)));
        assert_eq!(parse_color("#1F6FEB"), Some((0x1f, 0x6f, 0xeb)));
        assert_eq!(parse_color("1f6feb"), None);
        assert_eq!(parse_color("#1f6fe"), None);
    }

    #[test]
    fn the_pressed_color_is_half_way_to_white() {
        assert_eq!(lighter((0x1f, 0x6f, 0xeb)), (143, 183, 245));
        assert_eq!(lighter((255, 255, 255)), (255, 255, 255));
    }

    #[test]
    fn a_slug_is_safe_as_a_folder_name() {
        assert_eq!(slug("Slides 2!"), "slides-2");
        assert_eq!(slug("  --  "), "app");
        assert_eq!(slug("Page Turner"), "page-turner");
    }

    #[test]
    fn an_image_path_must_stay_inside_the_app_folder() {
        let app = Path::new("/apps/slides");
        assert_eq!(image_path(app, "images/a.png"), Some(app.join("images/a.png")));
        assert_eq!(image_path(app, "../other/a.png"), None);
        assert_eq!(image_path(app, "/etc/a.png"), None);
    }
}
