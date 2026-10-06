//! A button's label on the band: measured in the band's own font, broken into
//! lines at spaces, and drawn at one size for the whole app.
//!
//! The font probe of 2026-09-29 measured the band's labels: their widths match
//! Roboto 2.138 Medium within 0.45 px on average at size 100, and `text_size`
//! is the em size. So the editor reads that file's advance widths here
//! (`ui/fonts/roboto_medium.ttf`), with no kerning. A character the font does
//! not have, Chinese included, counts as one em, as the band draws it.
//!
//! The band's widths match Roboto Medium, but on the band's screen the letters
//! look as thin as Roboto Regular (the user's comparison on 2026-09-30). So the
//! preview draws labels in Regular, within 2% of the width they measure, and
//! the line breaks and the size still come from the measure here.

use crate::geometry::Rect;
use crate::layout::Layout;
use std::sync::OnceLock;

/// Roboto 2.138 Medium, whose widths the band's labels have.
static FONT_FILE: &[u8] = include_bytes!("../ui/fonts/roboto_medium.ttf");

/// The largest label, the existing apps' size.
pub const LARGEST: u32 = 34;

/// The smallest size that is easy to read on the band. ISO 9241-303 and
/// ANSI/HFES 100-2007 ask for capitals of at least 16 arcminutes: at 30 cm on
/// the Band 7's 326 PPI, 17.9 px. Roboto's capitals are 0.711 of the size.
pub const READABLE: u32 = 25;

/// The smallest size at all, when the app allows smaller text.
pub const SMALLEST: u32 = 10;

/// Room to spare across a button, in px, for its rounded ends. Down, a line's
/// own height already holds the font's space above and below the letters.
const SPARE: i64 = 8;

/// What the editor reads from the font, once.
struct Font {
    face: ttf_parser::Face<'static>,
    /// Font units to the em: 2048 for Roboto.
    em: u64,
    /// One line's height, in font units: the ascender plus the descender,
    /// (1900 + 500) / 2048 = 1.172 of the size for Roboto.
    line: u64,
}

fn font() -> &'static Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(|| {
        let face = ttf_parser::Face::parse(FONT_FILE, 0).expect("the shipped font parses");
        let em = u64::from(face.units_per_em());
        let line = (i32::from(face.ascender()) - i32::from(face.descender())) as u64;
        Font { face, em, line }
    })
}

/// A text's width on one line, in font units.
fn units(text: &str) -> u64 {
    let font = font();
    text.chars()
        .map(|c| {
            font.face
                .glyph_index(c)
                .and_then(|glyph| font.face.glyph_hor_advance(glyph))
                .map_or(font.em, u64::from)
        })
        .sum()
}

/// A label's width on one line at `size`, in px.
pub fn width(text: &str, size: u32) -> f64 {
    units(text) as f64 * f64::from(size) / font().em as f64
}

/// Whether text fits `room` px across at `size`. In whole numbers, so no
/// rounding decides a fit.
fn across(text: &str, size: u32, room: i64) -> bool {
    room >= 0 && units(text) * u64::from(size) <= room as u64 * font().em
}

/// Whether `count` lines fit `room` px down at `size`.
fn down(count: usize, size: u32, room: i64) -> bool {
    room >= 0 && count as u64 * font().line * u64::from(size) <= room as u64 * font().em
}

/// The lines a label takes at `size` on a button of `w` x `h`: the label as
/// it is when it fits across, or else its words, as few lines as fit, each
/// filled greedily. It never breaks inside a word, so a word wider than the
/// button takes a line of its own, and does not fit. Lines too many for the
/// button's height would be cut top and bottom, so then the label stays on
/// one line, and does not fit.
pub fn lines(label: &str, size: u32, w: i64, h: i64) -> Vec<String> {
    let room = w - SPARE;
    if across(label, size, room) || label.split_whitespace().next().is_none() {
        return vec![label.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    for word in label.split_whitespace() {
        match lines.last_mut() {
            Some(line) if across(&format!("{line} {word}"), size, room) => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    if down(lines.len(), size, h) {
        lines
    } else {
        vec![label.to_string()]
    }
}

/// Whether a label fits a button of `w` x `h` at `size`: every line across,
/// with 8 px to spare, and the lines' total height down.
pub fn fits(label: &str, size: u32, w: i64, h: i64) -> bool {
    let lines = lines(label, size, w, h);
    lines.iter().all(|line| across(line, size, w - SPARE)) && down(lines.len(), size, h)
}

/// The label as the band gets it at `size`: its lines, joined by "\n".
pub fn for_band(label: &str, size: u32, w: i64, h: i64) -> String {
    lines(label, size, w, h).join("\n")
}

/// The one size an app's labels are drawn at, and the buttons whose label
/// does not fit at it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sizing {
    pub size: u32,
    /// The smallest size the app may use: [`READABLE`], or [`SMALLEST`] when
    /// it allows smaller text.
    pub floor: u32,
    /// The buttons, counted from 0, whose label does not fit at `size`.
    pub misfits: Vec<usize>,
}

/// The app's size: the largest whole size, at most [`LARGEST`], at which
/// every label fits, each with its own line breaks, and never below the
/// floor. Picture buttons, buttons with no label or spaces only, which the
/// band draws as nothing, and buttons with no room do not count.
pub fn app_size(layout: &Layout, rects: &[Rect]) -> Sizing {
    let floor = if layout.small_text {
        SMALLEST
    } else {
        READABLE
    };
    let labels: Vec<(usize, &str, &Rect)> = layout
        .buttons
        .iter()
        .zip(rects)
        .enumerate()
        .filter(|(_, (button, rect))| {
            button.image.is_none() && !button.label.trim().is_empty() && rect.w > 0 && rect.h > 0
        })
        .map(|(i, (button, rect))| (i, button.label.as_str(), rect))
        .collect();
    let every = |size: u32| {
        labels
            .iter()
            .all(|(_, label, r)| fits(label, size, r.w, r.h))
    };
    let size = (floor..=LARGEST)
        .rev()
        .find(|&size| every(size))
        .unwrap_or(floor);
    let misfits = labels
        .iter()
        .filter(|(_, label, r)| !fits(label, size, r.w, r.h))
        .map(|(i, _, _)| *i)
        .collect();
    Sizing {
        size,
        floor,
        misfits,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::place;
    use crate::layout::Button;

    /// The widths the band gave the font probe for these strings at 34 px.
    /// The editor may measure up to 2 px wider, and at most 1 px narrower,
    /// so a label it says fits is not cut on the band.
    #[test]
    fn a_label_measures_as_the_band_measured_it() {
        let band = [
            ("Previous", 132.0),
            ("MUTE", 91.0),
            ("Home", 90.0),
            ("Next", 71.0),
            ("Blank", 84.0),
            ("Menu", 85.0),
            ("Zoom", 88.0),
        ];
        for (text, measured) in band {
            let ours = width(text, 34);
            assert!(
                ours <= measured + 2.0 && ours >= measured - 1.0,
                "{text}: {ours:.2} against the band's {measured}"
            );
        }
        // fontTools reads Roboto 2.138 Medium the same: 133.29 and 70.84.
        assert!((width("Previous", 34) - 133.29).abs() < 0.01);
        assert!((width("Next", 34) - 70.84).abs() < 0.01);
    }

    /// A character outside Roboto, Chinese included, is one em wide.
    #[test]
    fn a_chinese_character_is_as_wide_as_the_size() {
        assert_eq!(width("上一页", 30), 90.0);
        assert!((width("OK上", 20) - (width("OK", 20) + 20.0)).abs() < 1e-9);
    }

    /// A label too wide for one line breaks at spaces, into as few lines as
    /// fit, each filled greedily. A word stays whole.
    #[test]
    fn a_label_breaks_at_spaces_into_as_few_lines_as_fit() {
        assert_eq!(lines("Next slide", 34, 87, 134), ["Next", "slide"]);
        assert_eq!(
            lines("Go to next slide", 25, 87, 134),
            ["Go to", "next", "slide"]
        );
        assert_eq!(lines("Next", 34, 87, 134), ["Next"], "it fits as it is");
        assert_eq!(
            lines("Previous", 25, 87, 134),
            ["Previous"],
            "one word never breaks"
        );
        assert_eq!(for_band("Next slide", 34, 87, 134), "Next\nslide");
        assert_eq!(for_band("End show", 34, 180, 134), "End show");
    }

    /// A label whose lines are too many for the button's height stays on one
    /// line, which the band draws whole in height. "Slide 10" is too wide for
    /// one line on 87 px at 25 px, and a button 33 px tall has room for one
    /// line, not two, so two would be cut top and bottom. A word too wide
    /// still takes a line of its own when the lines fit down.
    #[test]
    fn a_label_too_tall_as_lines_stays_on_one_line() {
        assert_eq!(lines("Slide 10", 25, 87, 33), ["Slide 10"]);
        assert_eq!(lines("Slide 10", 25, 87, 134), ["Slide", "10"]);
        assert!(!fits("Slide 10", 25, 87, 33));
        assert_eq!(
            lines("Supercalifragilistic Next", 10, 87, 134),
            ["Supercalifragilistic", "Next"]
        );
    }

    /// Every line fits across, with 8 px to spare for the button's rounded
    /// ends, and the lines fit down. A line is 1.172 of the size, which holds
    /// the font's own space above and below the letters, so nothing more is
    /// spared down.
    #[test]
    fn a_label_fits_across_and_down() {
        assert!(fits("Previous", 20, 87, 134));
        assert!(!fits("Previous", 21, 87, 134), "82.3 px on 79 px of room");
        assert!(fits("Next slide", 34, 87, 134), "two lines, 79.7 px of 134");
        assert!(
            !fits("Go to next slide", 34, 87, 134),
            "four lines at 34 px"
        );
        assert!(
            fits("Go to next slide", 25, 87, 134),
            "three lines at 25 px"
        );
        // A button 33 px tall holds one line up to 28 px (32.8 px), not at
        // 29 px (34.0 px). So at 25 px, a line fits in its 29.3 px.
        assert!(fits("Go", 28, 87, 33));
        assert!(!fits("Go", 29, 87, 33));
        assert!(fits("Slide 1", 25, 87, 33));
        assert!(!fits("Supercalifragilistic", 10, 87, 134), "84 px at 10 px");
    }

    /// An app in two columns over a 60 px strip, a button for each label.
    fn app(labels: &[&str], small_text: bool) -> Layout {
        let mut l = Layout::new("slides");
        l.columns = 2;
        l.bottom_strip = 60;
        l.small_text = small_text;
        for (i, label) in labels.iter().enumerate() {
            let mut b = Button::new(&format!("bb{:02x}", i + 1), "#2f6fde");
            b.label = label.to_string();
            l.buttons.push(b);
        }
        l
    }

    /// The design's slides app: "End show" takes the last row, so the other
    /// four buttons are 87 x 134 px.
    fn design(small_text: bool) -> Layout {
        let mut l = app(
            &["Previous", "Next", "Laser", "Blank", "End show"],
            small_text,
        );
        l.buttons[4].span = true;
        l
    }

    fn sizing(l: &Layout) -> Sizing {
        app_size(l, &place(l))
    }

    /// "Previous" does not fit at 25 px, so the design's app stays at 25 px,
    /// and "Previous" is a misfit. With smaller text allowed, the app takes
    /// 20 px, where "Previous" fits. One size serves the whole app: "Blank"
    /// alone would take 31 px, and "Next" and "Back" take 34.
    #[test]
    fn one_size_for_the_app_never_below_25_px_unless_allowed() {
        let l = design(false);
        let rects = place(&l);
        assert_eq!((rects[0].w, rects[0].h), (87, 134));
        assert_eq!(
            app_size(&l, &rects),
            Sizing {
                size: 25,
                floor: 25,
                misfits: vec![0]
            }
        );
        assert_eq!(
            sizing(&design(true)),
            Sizing {
                size: 20,
                floor: 10,
                misfits: vec![]
            }
        );
        assert_eq!(sizing(&app(&["Next", "Blank"], false)).size, 31);
        assert_eq!(sizing(&app(&["Next", "Back"], false)).size, 34);
    }

    /// Two words that fit as two lines keep the app large, where shrinking
    /// them onto one line would take it down to 17 px.
    #[test]
    fn two_lines_keep_the_app_larger_than_shrinking_would() {
        assert_eq!(sizing(&app(&["Next slide", "Back"], true)).size, 34);
        assert!(width("Next slide", 18) > 79.0, "one line would need 17 px");
    }

    /// With smaller text allowed, a label that does not fit even at 10 px
    /// leaves the app at 10 px, and is a misfit.
    #[test]
    fn a_label_too_long_even_at_10_px_is_a_misfit_at_10_px() {
        assert_eq!(
            sizing(&app(&["Supercalifragilistic", "Next"], true)),
            Sizing {
                size: 10,
                floor: 10,
                misfits: vec![0]
            }
        );
    }

    /// A picture button has no label, and a button with no label, or with
    /// spaces only, has nothing the band draws, so none takes the size down.
    /// The review of 2026-09-30 found that 10 spaces took an app to 31 px.
    #[test]
    fn pictures_and_empty_labels_do_not_count() {
        let mut l = app(&["Previous", "", "Next", "          "], false);
        l.buttons[0].image = Some("images/laser.png".to_string());
        assert_eq!(
            sizing(&l),
            Sizing {
                size: 34,
                floor: 25,
                misfits: vec![]
            }
        );
    }
}
