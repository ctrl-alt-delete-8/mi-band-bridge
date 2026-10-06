//! What stops a build, and what the user may choose to ignore.
//!
//! An error is a layout that cannot be built, or one whose buttons BandBridge
//! cannot tell apart. Everything else is advice the user can dismiss, except
//! a setting card: it shows what a setting the user chose does, such as
//! smaller text, so it cannot be dismissed.
//!
//! The window shows each finding as a card: a title that says what is wrong
//! in a few words, then a message that says it in full, with what to do.

use crate::geometry::{self, MIN_BOTTOM_STRIP, MIN_TOUCH_MM};
use crate::images;
use crate::label::{self, READABLE, SMALLEST};
use crate::layout::{self, Button, Layout};
use crate::signals::Use;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
const JPEG_SIGNATURE: [u8; 3] = [0xff, 0xd8, 0xff];

/// Below this contrast with the black screen, a button may be hard to see.
/// 3 to 1 is the least contrast WCAG 2.1 asks of the parts of a control
/// (1.4.11, Non-text Contrast).
const MIN_CONTRAST: f64 = 3.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub key: String,
    /// The button the finding is about, counted from 0. The window's "Show
    /// button" opens it.
    pub button: Option<usize>,
    /// The button's field at fault, when one is: "signal", "color",
    /// "pressColor" or "image".
    pub field: Option<&'static str>,
    /// When buttons send one signal: each of them, in order, the first and
    /// then every button that repeats it. Empty otherwise.
    pub same_signal: Vec<usize>,
    /// What is wrong, in a few words.
    pub title: String,
    /// What is wrong in full, and what to do.
    pub message: String,
    /// The finding as the button's own row says it, where the message would
    /// name the button the row already shows.
    pub note: Option<String>,
    /// When buttons send one signal: what the first button's own row says,
    /// naming the buttons that repeat it.
    pub first_note: Option<String>,
    /// A button on the card that changes an app setting, when the card
    /// offers one.
    pub action: Option<Action>,
    pub dismissed: bool,
}

/// A card's button: its words, and the value it gives the app's smaller
/// text setting, the one setting a card changes.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub label: String,
    pub small_text: bool,
}

impl Finding {
    fn new(key: String, title: String, message: String) -> Finding {
        Finding {
            key,
            button: None,
            field: None,
            same_signal: Vec::new(),
            title,
            message,
            note: None,
            first_note: None,
            action: None,
            dismissed: false,
        }
    }

    /// With a button that turns smaller text on or off.
    fn offering(mut self, label: String, small_text: bool) -> Finding {
        self.action = Some(Action { label, small_text });
        self
    }

    /// About button `i`, and the field of it at fault, if one is.
    fn on(mut self, i: usize, field: Option<&'static str>) -> Finding {
        self.button = Some(i);
        self.field = field;
        self
    }

    fn noting(mut self, note: String) -> Finding {
        self.note = Some(note);
        self
    }

    /// Advice, which the user may have dismissed.
    fn advice(mut self, layout: &Layout) -> Finding {
        self.dismissed = layout.dismissed.iter().any(|k| k == &self.key);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Report {
    pub errors: Vec<Finding>,
    pub warnings: Vec<Finding>,
    /// Cards about a setting the user chose, such as smaller text. They are
    /// not advice: they cannot be dismissed, and no count includes them.
    pub settings: Vec<Finding>,
}

impl Report {
    pub fn can_build(&self) -> bool {
        self.errors.is_empty()
    }
}

/// A button as a sentence names it: button 4 “Blank”, or button 4 when it has
/// no label.
fn named(i: usize, button: &Button) -> String {
    match button.label.trim() {
        "" => format!("button {}", i + 1),
        label => format!("button {} “{label}”", i + 1),
    }
}

/// A button as a title names it: “Blank”, or button 4 when it has no label.
fn titled(i: usize, button: &Button) -> String {
    match button.label.trim() {
        "" => format!("button {}", i + 1),
        label => format!("“{label}”"),
    }
}

/// The text with its first letter in upper case, to start a sentence.
fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// How much light a color gives, as WCAG 2.1 measures it: 0 for black, 1 for
/// white.
fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
    let linear = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

/// Is a color too dark to stand out on the band's black screen? WCAG 2.1's
/// contrast of two colors, with black's luminance of 0, runs from 1 for black
/// to 21 for white.
fn is_dark(rgb: (u8, u8, u8)) -> bool {
    (luminance(rgb) + 0.05) / 0.05 < MIN_CONTRAST
}

/// A color as a person names it: black, dark gray, dark blue.
fn color_name((r, g, b): (u8, u8, u8)) -> String {
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    if max < 0x20 {
        return "black".to_string();
    }
    let dark = if max < 0xa0 { "dark " } else { "" };
    // Channels this close together read as gray, whatever their hue.
    let chroma = f64::from(max - min);
    if chroma * 4.0 < f64::from(max) {
        return format!("{dark}gray");
    }
    let (r, g, b, top) = (f64::from(r), f64::from(g), f64::from(b), f64::from(max));
    let sector = if top == r {
        ((g - b) / chroma).rem_euclid(6.0)
    } else if top == g {
        (b - r) / chroma + 2.0
    } else {
        (r - g) / chroma + 4.0
    };
    let angle = sector * 60.0;
    // Brown is a dark orange, or a red with some gray in it, as CSS's
    // saddlebrown and brown are.
    let brown = match angle {
        h if (15.0..45.0).contains(&h) => max < 0xa0,
        h if !(15.0..345.0).contains(&h) => chroma / top < 0.85 && max < 0xc0,
        _ => false,
    };
    if brown {
        return "brown".to_string();
    }
    // Magenta reads as purple, as CSS's purple and darkmagenta are.
    let hue = match angle {
        h if h < 15.0 => "red",
        h if h < 45.0 => "orange",
        h if h < 70.0 => "yellow",
        h if h < 160.0 => "green",
        h if h < 200.0 => "teal",
        h if h < 255.0 => "blue",
        h if h < 320.0 => "purple",
        h if h < 345.0 => "pink",
        _ => "red",
    };
    format!("{dark}{hue}")
}

/// How many buttons, as a title counts them: in words up to nine, then in
/// digits.
fn how_many(n: usize) -> String {
    const WORDS: [&str; 10] = [
        "No", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine",
    ];
    WORDS
        .get(n)
        .map_or_else(|| n.to_string(), |word| word.to_string())
}

/// "A and B", or "A, B and C".
fn listed(items: &[String]) -> String {
    match items.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => items.join(""),
    }
}

/// Why an image cannot be used, or None when it can.
fn image_problem(app_dir: &Path, rel: &str) -> Option<String> {
    let Some(path) = layout::image_path(app_dir, rel) else {
        return Some(format!("“{rel}” must be inside the app folder."));
    };
    let Ok(mut file) = std::fs::File::open(&path) else {
        return Some(format!("“{rel}” is missing."));
    };
    // The file's own first bytes decide, not its name.
    let mut head = [0u8; 8];
    if file.read_exact(&mut head).is_err() || (head != PNG_SIGNATURE && head[..3] != JPEG_SIGNATURE)
    {
        return Some(format!("“{rel}” is not a PNG or JPEG file."));
    }
    if let Err(reason) = images::load(&path) {
        return Some(format!("“{rel}” cannot be read: {reason}"));
    }
    None
}

/// What a signal is, in the button's own row: the design's words from its
/// first state. The row's Use button offers a signal that is free.
const SIGNAL_RULE: &str = "Each button sends one signal, from bb01 to bbff.";

/// What to do when a free strip or a button is too small.
const MAKE_ROOM: &str = "Remove buttons, or make the free strip smaller.";

/// The signal as the band sends it, when the text is one.
///
/// The band keeps 0x01, 0x02, and 0x03 on this endpoint for its own
/// messages, so an app that starts a signal with one of them talks over the
/// platform. Every app here starts at bb, which is well clear, and a byte
/// after it tells the buttons apart.
fn sendable(text: &str) -> Option<String> {
    let bytes = layout::parse_signal(text)?;
    (bytes.first() == Some(&layout::SIGNAL_PREFIX) && bytes.len() >= 2)
        .then(|| bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Buttons as a row's note lists them: button 2, “Next”, and button 4,
/// “Blank”. The comma before "and" closes a label, so a button with none
/// takes no comma: button 2 and button 3.
fn used_by(layout: &Layout, buttons: &[usize]) -> String {
    let items: Vec<(String, bool)> = buttons
        .iter()
        .map(|&i| match layout.buttons[i].label.trim() {
            "" => (format!("button {}", i + 1), false),
            label => (format!("button {}, “{label}”", i + 1), true),
        })
        .collect();
    match items.split_last() {
        Some(((last, _), rest)) if !rest.is_empty() => {
            let head: Vec<&str> = rest.iter().map(|(text, _)| text.as_str()).collect();
            let comma = if rest.last().is_some_and(|&(_, labeled)| labeled) {
                ","
            } else {
                ""
            };
            format!("{}{comma} and {last}", head.join(", "))
        }
        Some(((only, _), _)) => only.clone(),
        None => String::new(),
    }
}

/// The error for every button that sends one signal: `group` holds them in
/// order, the first and each button that repeats it. Its Show button opens
/// the first repeat. Each repeat's row names the first button, and the first
/// button's row names the repeats.
fn repeated(layout: &Layout, signal: &str, group: &[usize]) -> Finding {
    let (first, repeat) = (group[0], group[1]);
    let mut names: Vec<String> = group
        .iter()
        .map(|&i| named(i, &layout.buttons[i]))
        .collect();
    names[0] = sentence(&names[0]);
    let all = if group.len() == 2 { "both" } else { "all" };
    let note = |buttons: &[usize]| {
        format!(
            "{signal} is already used by {}. Each button needs its own signal.",
            used_by(layout, buttons)
        )
    };
    Finding {
        first_note: Some(note(&group[1..])),
        same_signal: group.to_vec(),
        ..Finding::new(
            format!("same-signal:{signal}"),
            format!("{} buttons send {signal}", how_many(group.len())),
            format!(
                "{} {all} send {signal}. Mi Band Bridge cannot tell them apart.",
                listed(&names)
            ),
        )
        .on(repeat, Some("signal"))
        .noting(note(&[first]))
    }
}

pub fn check(layout: &Layout, app_dir: &Path, used: &[Use]) -> Report {
    let mut report = Report::default();
    let own = app_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned());

    if layout::parse_color(&layout.color).is_none() {
        report.errors.push(Finding::new(
            "bad-color:app".to_string(),
            format!("“{}” is not a color", layout.color),
            "Write the app color as #rrggbb.".to_string(),
        ));
    }
    if let Some(icon) = &layout.icon {
        if let Some(problem) = image_problem(app_dir, icon) {
            report.errors.push(Finding::new(
                "bad-image:icon".to_string(),
                "The app icon cannot be used".to_string(),
                problem,
            ));
        }
    }
    if layout.bottom_strip < MIN_BOTTOM_STRIP {
        report.warnings.push(
            Finding::new(
                format!("bottom-strip:{}", layout.bottom_strip),
                "A back swipe may press a button".to_string(),
                format!(
                    "The free strip is {} px. Make it {MIN_BOTTOM_STRIP} px or more.",
                    layout.bottom_strip
                ),
            )
            .advice(layout),
        );
    }

    let rects = geometry::place(layout);
    // Every label is drawn at one size. Below 25 px, labels are hard to read,
    // which only an app that allows smaller text comes to.
    let sizing = label::app_size(layout, &rects);
    if sizing.size < READABLE {
        report.settings.push(
            Finding::new(
                format!("small-text:{}", sizing.size),
                "Smaller text is allowed".to_string(),
                format!(
                    "Labels are {} px, below the {READABLE} px that is easy to read.",
                    sizing.size
                ),
            )
            .offering(format!("Use {READABLE} px"), false),
        );
    }
    // The buttons that send each signal, in order. BandBridge knows a button
    // only by its signal, so buttons that send one are one button to it.
    let mut senders: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, button) in layout.buttons.iter().enumerate() {
        if let Some(signal) = sendable(&button.signal) {
            senders.entry(signal).or_default().push(i);
        }
    }
    for (i, (button, rect)) in layout.buttons.iter().zip(&rects).enumerate() {
        let n = i + 1;
        let signal = button.signal.trim();
        match sendable(signal) {
            None => {
                let title = if signal.is_empty() {
                    format!("{} has no signal", sentence(&titled(i, button)))
                } else {
                    format!("“{signal}” is not a signal")
                };
                report.errors.push(
                    Finding::new(
                        format!("bad-signal:{i}"),
                        title,
                        format!("Give button {n} a signal from bb01 to bbff."),
                    )
                    .on(i, Some("signal"))
                    .noting(SIGNAL_RULE.to_string()),
                );
            }
            // Buttons that send one signal get one card, where the first
            // repeat stands.
            Some(signal) => {
                let group = &senders[&signal];
                if group.len() > 1 && group[1] == i {
                    report.errors.push(repeated(layout, &signal, group));
                }
            }
        }
        let rgb = layout::parse_color(&button.color);
        if rgb.is_none() {
            report.errors.push(
                Finding::new(
                    format!("bad-color:{i}"),
                    format!("“{}” is not a color", button.color),
                    format!("Write the color of {} as #rrggbb.", named(i, button)),
                )
                .on(i, Some("color"))
                .noting("Write the color as #rrggbb.".to_string()),
            );
        }
        if let Some(press) = &button.press_color {
            if layout::parse_color(press).is_none() {
                report.errors.push(
                    Finding::new(
                        format!("bad-color:{i}:press"),
                        format!("“{press}” is not a color"),
                        format!(
                            "Write the pressed color of {} as #rrggbb.",
                            named(i, button)
                        ),
                    )
                    .on(i, Some("pressColor"))
                    .noting("Write the pressed color as #rrggbb.".to_string()),
                );
            }
        }
        if let Some(image) = &button.image {
            if let Some(problem) = image_problem(app_dir, image) {
                report.errors.push(
                    Finding::new(
                        format!("bad-image:{i}"),
                        format!("The picture of {} cannot be used", titled(i, button)),
                        problem,
                    )
                    .on(i, Some("image")),
                );
            }
        }
        if rect.w < 1 || rect.h < 1 {
            report.errors.push(
                Finding::new(
                    format!("no-space:{i}"),
                    format!("{} has no room", sentence(&titled(i, button))),
                    MAKE_ROOM.to_string(),
                )
                .on(i, None),
            );
            continue;
        }

        let (w_mm, h_mm) = (
            geometry::px_to_mm(rect.w as f64),
            geometry::px_to_mm(rect.h as f64),
        );
        if w_mm < MIN_TOUCH_MM || h_mm < MIN_TOUCH_MM {
            report.warnings.push(
                Finding::new(
                    format!("small-button:{i}:{}x{}", rect.w, rect.h),
                    format!("{} may be hard to press", sentence(&titled(i, button))),
                    format!(
                        "Button {n} is {w_mm:.1} × {h_mm:.1} mm, and a finger needs {MIN_TOUCH_MM:.0} mm each way. {MAKE_ROOM}"
                    ),
                )
                .on(i, None)
                .advice(layout),
            );
        }
        // A label that does not fit even at the smallest size the app allows.
        // The band cuts it. A picture replaces the label, so it is never one.
        if sizing.misfits.contains(&i) {
            let message = if layout.small_text {
                format!("Not even at {SMALLEST} px.")
            } else {
                format!(
                    "Not even at {READABLE} px, the smallest size that is easy to read on the band."
                )
            };
            let mut fit = Finding::new(
                format!(
                    "label-fit:{}:{i}:{}x{}:{}",
                    sizing.floor, rect.w, rect.h, button.label
                ),
                format!("{} does not fit", sentence(&titled(i, button))),
                message,
            )
            .on(i, None)
            .advice(layout);
            if !layout.small_text {
                fit = fit.offering("Allow smaller text".to_string(), true);
            }
            report.warnings.push(fit);
        }
        if let Some(signal) = layout::normalize_signal(&button.signal) {
            for other in used
                .iter()
                .filter(|u| u.signal == signal && u.dir.as_deref() != own.as_deref())
            {
                report.warnings.push(
                    Finding::new(
                        format!("signal-clash:{i}:{signal}:{}", other.app),
                        format!("Another app sends {signal}"),
                        format!(
                            "{} and {} both send {signal}. Mi Band Bridge cannot tell which one sent it.",
                            sentence(&named(i, button)),
                            other.app
                        ),
                    )
                    .on(i, Some("signal"))
                    .advice(layout),
                );
            }
        }
        // The screen is black, so a dark button all but disappears. A picture
        // covers the whole button, so then its color does not show.
        if let (Some(rgb), None) = (rgb, &button.image)
            && is_dark(rgb)
        {
            let (r, g, b) = rgb;
            report.warnings.push(
                Finding::new(
                    format!("dark-button:{i}:#{r:02x}{g:02x}{b:02x}"),
                    format!("{} may be hard to see", sentence(&titled(i, button))),
                    format!(
                        "Button {n} is {} on the black screen. Pick a lighter color so it stands out.",
                        color_name(rgb)
                    ),
                )
                .on(i, Some("color"))
                .advice(layout),
            );
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Button;
    use crate::signals::Use;

    fn two_buttons() -> Layout {
        let mut l = Layout::new("Slides");
        l.buttons.push(Button::new("bb04", "#1f6feb"));
        l.buttons.push(Button::new("bb05", "#1f6feb"));
        l
    }

    fn run(layout: &Layout) -> Report {
        let dir = tempfile::tempdir().unwrap();
        check(layout, &dir.path().join("slides"), &[])
    }

    #[test]
    fn a_plain_two_button_app_is_clean() {
        let report = run(&two_buttons());
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert!(report.can_build());
    }

    /// Shrinking the strip is a choice the editor offers, so it only warns
    /// well below the default. See MIN_BOTTOM_STRIP.
    #[test]
    fn a_strip_above_the_threshold_is_silent() {
        let mut l = two_buttons();
        l.bottom_strip = 40;
        let report = run(&l);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        l.bottom_strip = 30;
        assert!(run(&l).warnings.is_empty(), "the threshold itself is silent");
    }

    #[test]
    fn a_small_strip_warns_and_can_be_dismissed() {
        let mut l = two_buttons();
        l.bottom_strip = 20;
        let report = run(&l);
        assert_eq!(report.warnings[0].key, "bottom-strip:20");
        assert_eq!(report.warnings[0].title, "A back swipe may press a button");
        assert_eq!(
            report.warnings[0].message,
            "The free strip is 20 px. Make it 30 px or more."
        );
        assert!(!report.warnings[0].dismissed);
        assert!(report.can_build(), "a warning never blocks the build");

        l.dismissed.push("bottom-strip:20".to_string());
        assert!(run(&l).warnings[0].dismissed);

        l.bottom_strip = 10;
        assert!(!run(&l).warnings[0].dismissed, "a new value brings it back");
    }

    /// The band keeps 01, 02, and 03 on this endpoint for its own messages,
    /// so every signal starts at bb, well clear of them.
    #[test]
    fn a_signal_must_start_with_bb() {
        let mut l = two_buttons();
        for bad in ["0201", "02", "c0ffee", "bb", "ff04"] {
            l.buttons[0].signal = bad.to_string();
            let report = run(&l);
            let found = report.errors.iter().find(|e| e.key == "bad-signal:0");
            let message = &found
                .unwrap_or_else(|| panic!("{bad} should be refused"))
                .message;
            assert_eq!(
                message, "Give button 1 a signal from bb01 to bbff.",
                "{bad}"
            );
        }
        for good in ["bb04", "BB1F", "bb0401"] {
            l.buttons[0].signal = good.to_string();
            let report = run(&l);
            assert!(
                !report.errors.iter().any(|e| e.key == "bad-signal:0"),
                "{good} should be accepted: {:?}",
                report.errors
            );
        }
    }

    /// Two columns give buttons 6.8 mm wide. That is a choice the editor
    /// offers, so taking it must not warn: see MIN_TOUCH_MM.
    #[test]
    fn two_columns_do_not_warn_about_width() {
        let mut l = two_buttons();
        l.columns = 2;
        let report = run(&l);
        let small: Vec<_> = report.warnings.iter().filter(|w| w.key.starts_with("small-button:")).collect();
        assert!(small.is_empty(), "{small:?}");
    }

    /// The warning still has to fire for a button that is genuinely too small.
    #[test]
    fn a_button_under_five_millimetres_warns() {
        let mut l = Layout::new("Many");
        for i in 0..7 {
            l.buttons
                .push(Button::new(&format!("bb{:02x}", i + 4), "#000000"));
        }
        let report = run(&l);
        let small: Vec<_> = report
            .warnings
            .iter()
            .filter(|w| w.key.starts_with("small-button:"))
            .collect();
        assert_eq!(small.len(), 7, "{small:?}");
        assert_eq!(
            small[0].message,
            "Button 1 is 14.0 × 3.9 mm, and a finger needs 5 mm each way. Remove buttons, or make the free strip smaller."
        );
    }

    #[test]
    fn many_buttons_warn_then_fail() {
        let mut l = Layout::new("Many");
        for i in 0..6 {
            l.buttons
                .push(Button::new(&format!("bb{:02x}", i + 4), "#000000"));
        }
        let report = run(&l);
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.key.starts_with("small-button:"))
        );
        assert!(report.can_build());

        for i in 6..60 {
            l.buttons
                .push(Button::new(&format!("bb{:02x}", i + 4), "#000000"));
        }
        let report = run(&l);
        let full = report
            .errors
            .iter()
            .find(|e| e.key.starts_with("no-space:"))
            .unwrap();
        assert!(full.title.ends_with(" has no room"), "{full:?}");
        assert_eq!(
            full.message,
            "Remove buttons, or make the free strip smaller."
        );
        assert!(!report.can_build());
    }

    #[test]
    fn a_bad_signal_or_color_is_an_error() {
        let mut l = two_buttons();
        l.buttons[0].signal = "xyz".to_string();
        l.buttons[1].color = "blue".to_string();
        l.buttons[1].press_color = Some("red".to_string());
        let report = run(&l);
        assert!(report.errors.iter().any(|e| e.key == "bad-signal:0"));
        let color = report
            .errors
            .iter()
            .find(|e| e.key == "bad-color:1")
            .unwrap();
        assert_eq!(color.note.as_deref(), Some("Write the color as #rrggbb."));
        let press = report
            .errors
            .iter()
            .find(|e| e.key == "bad-color:1:press")
            .unwrap();
        assert_eq!(
            press.note.as_deref(),
            Some("Write the pressed color as #rrggbb.")
        );
    }

    /// The editor reads JPEG as well as PNG, and judges a file by its own
    /// first bytes, not by its name.
    #[test]
    fn a_jpeg_is_accepted_whatever_it_is_called() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("slides");
        std::fs::create_dir_all(app.join("images")).unwrap();
        let jpeg = crate::images::tests::jpeg_bytes();
        std::fs::write(app.join("images/photo.jpg"), &jpeg).unwrap();
        std::fs::write(app.join("images/misnamed.png"), &jpeg).unwrap();
        let mut l = two_buttons();
        l.buttons[0].image = Some("images/photo.jpg".to_string());
        l.buttons[1].image = Some("images/misnamed.png".to_string());
        let report = check(&l, &app, &[]);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }

    #[test]
    fn a_missing_or_unreadable_image_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("slides");
        std::fs::create_dir_all(app.join("images")).unwrap();
        std::fs::write(app.join("images/fake.png"), b"not a png").unwrap();
        let mut l = two_buttons();
        l.buttons[0].image = Some("images/gone.png".to_string());
        l.buttons[1].image = Some("images/fake.png".to_string());
        let report = check(&l, &app, &[]);
        let messages: Vec<_> = report.errors.iter().map(|e| e.message.clone()).collect();
        assert!(messages.iter().any(|m| m.contains("missing")), "{messages:?}");
        assert!(messages.iter().any(|m| m.contains("not a PNG or JPEG")), "{messages:?}");
    }

    #[test]
    fn a_png_with_the_right_signature_but_a_broken_body_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("slides");
        std::fs::create_dir_all(app.join("images")).unwrap();
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.extend_from_slice(b"this is not a real PNG body");
        std::fs::write(app.join("images/broken.png"), &bytes).unwrap();
        let mut l = two_buttons();
        l.buttons[0].image = Some("images/broken.png".to_string());
        let report = check(&l, &app, &[]);
        let err = report.errors.iter().find(|e| e.key == "bad-image:0").expect("a bad-image:0 error");
        assert!(err.message.contains("cannot be read"), "{}", err.message);
    }

    #[test]
    fn a_missing_icon_is_an_error() {
        let mut l = two_buttons();
        l.icon = Some("images/gone.png".to_string());
        let report = run(&l);
        assert!(report.errors.iter().any(|e| e.key == "bad-image:icon"), "{:?}", report.errors);
    }

    #[test]
    fn a_bad_app_color_is_an_error() {
        let mut l = two_buttons();
        l.color = "blue".to_string();
        let report = run(&l);
        assert!(report.errors.iter().any(|e| e.key == "bad-color:app"), "{:?}", report.errors);
    }

    #[test]
    fn a_bad_pressed_color_is_an_error() {
        let mut l = two_buttons();
        l.buttons[0].press_color = Some("blue".to_string());
        let report = run(&l);
        assert!(report.errors.iter().any(|e| e.key == "bad-color:0:press"), "{:?}", report.errors);
    }

    /// A label breaks at spaces, so a long label of short words takes more
    /// lines, and fits.
    #[test]
    fn a_long_label_breaks_into_lines_and_fits() {
        let mut l = two_buttons();
        l.buttons[0].label = "A label far too long for the band".to_string();
        let report = run(&l);
        let fits = found(&report, "label-fit:");
        assert!(fits.is_empty(), "{fits:?}");
    }

    /// The design's slide clicker as the design lays it out: two columns,
    /// the last button full width, and a free strip of 60 px. Its buttons are
    /// 87 × 134 px.
    fn design_slides() -> Layout {
        let mut l = slides(["bb01", "bb02", "bb03", "bb04", "bb05"]);
        l.columns = 2;
        l.bottom_strip = 60;
        l.buttons[4].span = true;
        l
    }

    fn found<'a>(report: &'a Report, kind: &str) -> Vec<&'a Finding> {
        report
            .warnings
            .iter()
            .filter(|w| w.key.starts_with(kind))
            .collect()
    }

    /// "Previous" does not fit the design's 87 px buttons even at 25 px, the
    /// smallest size that is easy to read on the band. Its card offers
    /// smaller text. "Next", "Laser" and "Blank" fit at 25 px, so they have
    /// no card.
    #[test]
    fn a_label_that_does_not_fit_at_25_px_offers_smaller_text() {
        let report = run(&design_slides());
        let fits = found(&report, "label-fit:");
        assert_eq!(fits.len(), 1, "{fits:?}");
        let fit = fits[0];
        assert_eq!(fit.button, Some(0));
        assert_eq!(fit.title, "“Previous” does not fit");
        assert_eq!(
            fit.message,
            "Not even at 25 px, the smallest size that is easy to read on the band."
        );
        assert_eq!(
            fit.action,
            Some(Action {
                label: "Allow smaller text".to_string(),
                small_text: true
            })
        );
        assert!(found(&report, "small-text:").is_empty());

        // Its Dismiss works as every warning's does.
        let mut l = design_slides();
        l.dismissed.push(fit.key.clone());
        assert!(found(&run(&l), "label-fit:")[0].dismissed);
    }

    /// With smaller text allowed, the design's app takes 20 px, where
    /// "Previous" fits. A card says the labels are below the easy size, and
    /// offers 25 px again. It shows a setting the person chose, not advice,
    /// so it is no warning, and a Dismiss cannot hide its button. When every
    /// label fits at 25 px or more, smaller text changes nothing, and there
    /// is no card.
    #[test]
    fn smaller_text_has_a_setting_card_that_offers_25_px_again() {
        let mut l = design_slides();
        l.small_text = true;
        let report = run(&l);
        assert!(found(&report, "label-fit:").is_empty());
        assert!(found(&report, "small-text:").is_empty(), "not a warning");
        assert_eq!(report.settings.len(), 1, "{:?}", report.settings);
        let small = &report.settings[0];
        assert_eq!(small.key, "small-text:20");
        assert_eq!(small.button, None);
        assert_eq!(small.title, "Smaller text is allowed");
        assert_eq!(
            small.message,
            "Labels are 20 px, below the 25 px that is easy to read."
        );
        assert_eq!(
            small.action,
            Some(Action {
                label: "Use 25 px".to_string(),
                small_text: false
            })
        );

        l.dismissed.push(small.key.clone());
        assert!(!run(&l).settings[0].dismissed);

        l.buttons[0].label = "Back".to_string();
        assert!(run(&l).settings.is_empty());
    }

    /// A label that does not fit even at 10 px, the smallest size at all,
    /// has a card with nothing more to offer. Its key differs from the 25 px
    /// card's, so a Dismiss of one leaves the other.
    #[test]
    fn a_label_that_does_not_fit_at_10_px_says_so() {
        let mut l = design_slides();
        l.buttons[0].label = "Supercalifragilistic".to_string();
        let at_25 = found(&run(&l), "label-fit:")[0].key.clone();
        l.small_text = true;
        let report = run(&l);
        let fit = found(&report, "label-fit:")[0];
        assert_eq!(fit.title, "“Supercalifragilistic” does not fit");
        assert_eq!(fit.message, "Not even at 10 px.");
        assert_eq!(fit.action, None);
        assert_ne!(fit.key, at_25);
        assert_eq!(
            report.settings[0].message,
            "Labels are 10 px, below the 25 px that is easy to read."
        );
    }

    /// A label of spaces only draws nothing on the band, so it gets no card,
    /// and it takes no label's size down. The review of 2026-09-30 found that
    /// 13 spaces made the card "Button 1 does not fit", and that 20 spaces
    /// with smaller text took every label to 15 px.
    #[test]
    fn a_label_of_spaces_counts_for_nothing() {
        let mut l = design_slides();
        l.buttons[0].label = " ".repeat(13);
        let report = run(&l);
        let fits = found(&report, "label-fit:");
        assert!(fits.is_empty(), "{fits:?}");

        l.buttons[0].label = " ".repeat(20);
        l.small_text = true;
        let report = run(&l);
        assert!(report.settings.is_empty(), "{:?}", report.settings);
    }

    /// The window reads a card's button from the report: its words, and the
    /// value it gives smallText. A card with no button of its own has none.
    #[test]
    fn a_card_s_button_reaches_the_window_as_json() {
        let report = run(&design_slides());
        let json = serde_json::to_value(found(&report, "label-fit:")[0]).unwrap();
        assert_eq!(
            json["action"],
            serde_json::json!({ "label": "Allow smaller text", "smallText": true })
        );
        let mut dark = two_buttons();
        dark.buttons[0].color = "#000000".to_string();
        let json = serde_json::to_value(&run(&dark).warnings[0]).unwrap();
        assert_eq!(json["action"], serde_json::Value::Null);

        let mut small = design_slides();
        small.small_text = true;
        let json = serde_json::to_value(run(&small)).unwrap();
        assert_eq!(
            json["settings"][0]["action"],
            serde_json::json!({ "label": "Use 25 px", "smallText": false })
        );
    }

    /// The design's slide clicker: five buttons, one of them a picture.
    fn slides(signals: [&str; 5]) -> Layout {
        let mut l = Layout::new("slides");
        let labels = ["Previous", "Next", "Laser", "Blank", "End show"];
        let colors = ["#2f6fde", "#1f9d55", "#d93a2f", "#3a3d44", "#c2410c"];
        for i in 0..5 {
            let mut b = Button::new(signals[i], colors[i]);
            b.label = labels[i].to_string();
            l.buttons.push(b);
        }
        l
    }

    /// BandBridge knows a button only by its signal, so two buttons that send
    /// one signal are one button to it. The later one carries the error, and
    /// names the earlier one.
    #[test]
    fn two_buttons_with_one_signal_stop_the_build() {
        let report = run(&slides(["bb01", "bb02", "bb03", " BB02", "bb05"]));
        let same: Vec<_> = report
            .errors
            .iter()
            .filter(|e| e.key.starts_with("same-signal:"))
            .collect();
        assert_eq!(same.len(), 1, "{:?}", report.errors);
        let error = same[0];
        assert_eq!(error.button, Some(3));
        assert_eq!(error.same_signal, [1, 3]);
        assert_eq!(error.field, Some("signal"));
        assert_eq!(error.title, "Two buttons send bb02");
        assert_eq!(
            error.message,
            "Button 2 “Next” and button 4 “Blank” both send bb02. Mi Band Bridge cannot tell them apart."
        );
        assert_eq!(
            error.note.as_deref(),
            Some("bb02 is already used by button 2, “Next”. Each button needs its own signal.")
        );
        // The first button's row names the button that repeats it.
        assert_eq!(
            error.first_note.as_deref(),
            Some("bb02 is already used by button 4, “Blank”. Each button needs its own signal.")
        );
        assert!(!report.can_build());
    }

    /// Three buttons with one signal are one error, one card, which names
    /// them all. Its Show button opens the first repeat, and each repeat's
    /// row names the first button.
    #[test]
    fn three_buttons_with_one_signal_are_one_error() {
        let report = run(&slides(["bb02", "bb02", "bb03", "bb02", "bb05"]));
        assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
        let error = &report.errors[0];
        assert_eq!(error.title, "Three buttons send bb02");
        assert_eq!(
            error.message,
            "Button 1 “Previous”, button 2 “Next” and button 4 “Blank” all send bb02. Mi Band Bridge cannot tell them apart."
        );
        assert_eq!(error.same_signal, [0, 1, 3]);
        assert_eq!(error.button, Some(1));
        assert_eq!(
            error.note.as_deref(),
            Some("bb02 is already used by button 1, “Previous”. Each button needs its own signal.")
        );
        assert_eq!(
            error.first_note.as_deref(),
            Some(
                "bb02 is already used by button 2, “Next”, and button 4, “Blank”. Each button needs its own signal."
            )
        );
    }

    /// A button with no label is named by its number alone, and the list
    /// takes a comma before "and" only to close a label.
    #[test]
    fn the_first_button_names_repeats_with_or_without_labels() {
        let first_note = |labels: [&str; 3]| {
            let mut l = Layout::new("Three");
            for label in labels {
                let mut b = Button::new("bb04", layout::DEFAULT_COLOR);
                b.label = label.to_string();
                l.buttons.push(b);
            }
            run(&l).errors[0].first_note.clone().unwrap()
        };
        assert_eq!(
            first_note(["A", "", ""]),
            "bb04 is already used by button 2 and button 3. Each button needs its own signal."
        );
        assert_eq!(
            first_note(["A", "B", ""]),
            "bb04 is already used by button 2, “B”, and button 3. Each button needs its own signal."
        );
        assert_eq!(
            first_note(["A", "", "C"]),
            "bb04 is already used by button 2 and button 3, “C”. Each button needs its own signal."
        );
    }

    /// A title counts in words up to nine, then in digits.
    #[test]
    fn many_buttons_with_one_signal_are_counted_in_the_title() {
        for (count, title) in [(9, "Nine buttons send bb04"), (10, "10 buttons send bb04")] {
            let mut l = Layout::new("Many");
            for _ in 0..count {
                l.buttons.push(Button::new("bb04", layout::DEFAULT_COLOR));
            }
            let report = run(&l);
            let same: Vec<_> = report
                .errors
                .iter()
                .filter(|e| e.key.starts_with("same-signal:"))
                .collect();
            assert_eq!(same.len(), 1, "{count}");
            assert_eq!(same[0].title, title);
            assert_eq!(same[0].same_signal.len(), count);
        }
    }

    /// A signal that cannot be read has its own error, and is not a repeat.
    #[test]
    fn a_signal_that_cannot_be_read_is_no_repeat() {
        for bad in ["xyz", "0201"] {
            let report = run(&slides([bad, bad, "bb03", "bb04", "bb05"]));
            assert!(
                !report
                    .errors
                    .iter()
                    .any(|e| e.key.starts_with("same-signal:")),
                "{bad}: {:?}",
                report.errors
            );
            assert_eq!(
                report
                    .errors
                    .iter()
                    .filter(|e| e.key.starts_with("bad-signal:"))
                    .count(),
                2,
                "{bad}"
            );
        }
    }

    /// The band's screen is black, so a dark button nearly disappears.
    #[test]
    fn a_dark_button_may_be_hard_to_see() {
        let mut l = slides(["bb01", "bb02", "bb03", "bb04", "bb05"]);
        let report = run(&l);
        let dark: Vec<_> = report
            .warnings
            .iter()
            .filter(|w| w.key.starts_with("dark-button:"))
            .collect();
        assert_eq!(dark.len(), 1, "only Blank is dark: {:?}", report.warnings);
        assert_eq!(dark[0].key, "dark-button:3:#3a3d44");
        assert_eq!(dark[0].button, Some(3));
        assert_eq!(dark[0].field, Some("color"));
        assert_eq!(dark[0].title, "“Blank” may be hard to see");
        assert_eq!(
            dark[0].message,
            "Button 4 is dark gray on the black screen. Pick a lighter color so it stands out."
        );
        assert!(report.can_build(), "a warning never blocks the build");

        l.dismissed.push("dark-button:3:#3a3d44".to_string());
        assert!(
            run(&l)
                .warnings
                .iter()
                .any(|w| w.key.starts_with("dark-button:") && w.dismissed)
        );

        l.buttons[3].color = "#000000".to_string();
        let report = run(&l);
        let dark = report
            .warnings
            .iter()
            .find(|w| w.key.starts_with("dark-button:"))
            .unwrap();
        assert!(!dark.dismissed, "another color brings it back");
        assert!(dark.message.contains(" is black "), "{}", dark.message);
    }

    /// A picture covers the whole button, so its color does not show.
    #[test]
    fn a_picture_button_is_not_judged_by_its_color() {
        let mut l = slides(["bb01", "bb02", "bb03", "bb04", "bb05"]);
        l.buttons[3].image = Some("images/blank.png".to_string());
        let report = run(&l);
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.key.starts_with("dark-button:")),
            "{:?}",
            report.warnings
        );
    }

    /// The contrast rule is WCAG 2.1's for the parts of a control, 3 to 1,
    /// against the black screen. The editor's own default color passes.
    #[test]
    fn only_a_color_under_three_to_one_on_black_is_dark() {
        for light in [
            layout::DEFAULT_COLOR,
            "#2f6fde",
            "#1f9d55",
            "#d93a2f",
            "#c2410c",
            "#ff0000",
            "#5e5e5e",
        ] {
            assert!(!is_dark(layout::parse_color(light).unwrap()), "{light}");
        }
        for dark in ["#000000", "#3a3d44", "#0000ff", "#8b0000", "#555555"] {
            assert!(is_dark(layout::parse_color(dark).unwrap()), "{dark}");
        }
    }

    #[test]
    fn a_color_is_named_as_a_person_says_it() {
        let name = |hex: &str| color_name(layout::parse_color(hex).unwrap());
        assert_eq!(name("#000000"), "black");
        assert_eq!(name("#3a3d44"), "dark gray");
        assert_eq!(name("#00007f"), "dark blue");
        assert_eq!(name("#0000ff"), "blue");
        assert_eq!(name("#5a0000"), "dark red");
        assert_eq!(name("#003c00"), "dark green");
        assert_eq!(name("#4b0082"), "dark purple");
    }

    /// Every CSS named color under 3 to 1 on black, as the warning names it.
    /// Magenta hues read as purple, and dark red-to-orange hues as brown.
    #[test]
    fn the_css_colors_the_check_flags_have_plain_names() {
        let flagged = [
            ("black", "#000000", "black"),
            ("blue", "#0000ff", "blue"),
            ("brown", "#a52a2a", "brown"),
            ("darkblue", "#00008b", "dark blue"),
            ("darkgreen", "#006400", "dark green"),
            ("darkmagenta", "#8b008b", "dark purple"),
            ("darkred", "#8b0000", "dark red"),
            ("darkslateblue", "#483d8b", "dark blue"),
            ("darkslategray", "#2f4f4f", "dark teal"),
            ("indigo", "#4b0082", "dark purple"),
            ("maroon", "#800000", "dark red"),
            ("mediumblue", "#0000cd", "blue"),
            ("midnightblue", "#191970", "dark blue"),
            ("navy", "#000080", "dark blue"),
            ("purple", "#800080", "dark purple"),
            ("rebeccapurple", "#663399", "dark purple"),
            ("saddlebrown", "#8b4513", "brown"),
        ];
        for (css, hex, name) in flagged {
            let rgb = layout::parse_color(hex).unwrap();
            assert!(is_dark(rgb), "{css} is flagged");
            assert_eq!(color_name(rgb), name, "{css}");
        }
        // The nearest CSS colors above the line are not flagged.
        for (css, hex) in [
            ("dimgray", "#696969"),
            ("firebrick", "#b22222"),
            ("sienna", "#a0522d"),
            ("darkviolet", "#9400d3"),
            ("green", "#008000"),
            ("darkolivegreen", "#556b2f"),
        ] {
            assert!(!is_dark(layout::parse_color(hex).unwrap()), "{css}");
        }
    }

    /// Each finding is a card in the window: a title, then the message.
    #[test]
    fn every_finding_has_a_title_and_a_message() {
        let mut l = slides(["bb01", "xyz", "0201", "", "bb01"]);
        l.color = "blue".to_string();
        l.icon = Some("images/gone.png".to_string());
        l.bottom_strip = 10;
        l.buttons[0].color = "#000000".to_string();
        l.buttons[1].color = "blue".to_string();
        l.buttons[2].press_color = Some("blue".to_string());
        l.buttons[4].image = Some("images/gone.png".to_string());
        l.buttons[0].label = "A label far too long for the band".to_string();
        let used = vec![Use {
            signal: "bb01".into(),
            app: "Other".into(),
            dir: Some("other".into()),
        }];
        let dir = tempfile::tempdir().unwrap();
        let report = check(&l, &dir.path().join("slides"), &used);

        let rows = |count: usize| {
            let mut many = Layout::new("Many");
            for i in 0..count {
                many.buttons.push(Button::new(
                    &format!("bb{:02x}", i + 4),
                    layout::DEFAULT_COLOR,
                ));
            }
            run(&many)
        };
        let (small, crowded) = (rows(7), rows(60));
        let mut smaller = design_slides();
        smaller.small_text = true;
        let smaller = run(&smaller);

        let findings: Vec<_> = [&report, &small, &crowded, &smaller]
            .into_iter()
            .flat_map(|r| r.errors.iter().chain(&r.warnings).chain(&r.settings))
            .collect();
        let kinds: std::collections::BTreeSet<_> = findings
            .iter()
            .map(|f| f.key.split(':').next().unwrap().to_string())
            .collect();
        for kind in [
            "bad-color",
            "bad-image",
            "bottom-strip",
            "bad-signal",
            "same-signal",
            "no-space",
            "small-button",
            "label-fit",
            "small-text",
            "signal-clash",
            "dark-button",
        ] {
            assert!(kinds.contains(kind), "{kind} is missing from {kinds:?}");
        }
        for f in findings {
            assert!(!f.title.is_empty() && !f.message.is_empty(), "{f:?}");
            assert!(
                !f.message.contains('"'),
                "curly quotes, as the window writes them: {f:?}"
            );
        }
        let empty = report
            .errors
            .iter()
            .find(|e| e.key == "bad-signal:3")
            .unwrap();
        assert_eq!(empty.title, "“Blank” has no signal");
    }

    /// Every signal error asks for a signal in the range the design gives,
    /// and says in the button's own row what a signal is. The row's Use
    /// button offers one that is free.
    #[test]
    fn a_signal_error_asks_for_one_from_bb01_to_bbff() {
        let report = run(&slides(["bb01", "xyz", "0201", "", "bb05"]));
        for (key, title, message) in [
            (
                "bad-signal:1",
                "“xyz” is not a signal",
                "Give button 2 a signal from bb01 to bbff.",
            ),
            (
                "bad-signal:2",
                "“0201” is not a signal",
                "Give button 3 a signal from bb01 to bbff.",
            ),
            (
                "bad-signal:3",
                "“Blank” has no signal",
                "Give button 4 a signal from bb01 to bbff.",
            ),
        ] {
            let error = report.errors.iter().find(|e| e.key == key).unwrap();
            assert_eq!(error.title, title);
            assert_eq!(error.message, message);
            assert_eq!(error.field, Some("signal"));
            assert_eq!(
                error.note.as_deref(),
                Some("Each button sends one signal, from bb01 to bbff.")
            );
        }
    }

    #[test]
    fn a_clash_warns_but_not_with_the_app_itself() {
        let l = two_buttons();
        let used = vec![
            Use { signal: "bb04".into(), app: "Slides".into(), dir: Some("slides".into()) },
            Use { signal: "bb05".into(), app: "Other".into(), dir: Some("other".into()) },
        ];
        let dir = tempfile::tempdir().unwrap();
        let report = check(&l, &dir.path().join("slides"), &used);
        let clashes: Vec<_> = report.warnings.iter().filter(|w| w.key.starts_with("signal-clash:")).collect();
        assert_eq!(clashes.len(), 1);
        assert_eq!(clashes[0].key, "signal-clash:1:bb05:Other");
    }
}
