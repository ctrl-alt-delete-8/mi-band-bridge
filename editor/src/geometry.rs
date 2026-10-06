//! Where buttons go on the band's screen, and how big they are.

/// The Xiaomi Smart Band 7 screen, in pixels. Measured on the band.
pub const SCREEN_W: u32 = 192;
pub const SCREEN_H: u32 = 490;

/// Pixel density, from Xiaomi's specification.
pub const PPI: f64 = 326.0;

/// Bare screen at the left, right, and top edges. From the existing apps.
pub const MARGIN: u32 = 6;

/// Space between rows of buttons. From the existing apps.
pub const ROW_GAP: u32 = 10;

/// Space between two columns of buttons.
pub const COL_GAP: u32 = 6;

/// The app icon size, from zmake's device profile for this band.
pub const ICON: u32 = 100;

/// The corner of a button when the app chose Rounded. 16 px is 1.2 mm, small
/// enough to read as a rectangle and large enough not to look like an error.
pub const ROUNDED_RADIUS: i64 = 16;

/// The screen's corner radius, from zmake's device profile for this band. It
/// is half the width, so the screen is a capsule: both ends are half circles.
/// A pixel outside that capsule is not on the screen.
pub const SCREEN_RADIUS: u32 = 96;

/// The corner radius of the safe area: the screen capsule, inset by MARGIN on
/// every side. The inset keeps the arc centers, so the gap from the safe area
/// to the glass is MARGIN the whole way round, at the ends as at the sides.
pub const SAFE_RADIUS: u32 = SCREEN_RADIUS - MARGIN;

/// The free strip at the bottom, where a back swipe starts.
pub const DEFAULT_BOTTOM_STRIP: u32 = 70;

/// Below this the editor warns about the free strip. It is well under the
/// default, because shrinking the strip is a choice the editor offers, and a
/// warning on every use of a choice is noise. 30 px is 2.3 mm, about the width
/// of a finger's first contact, so a swipe that starts there can still miss
/// every button.
pub const MIN_BOTTOM_STRIP: u32 = 30;

/// Below this size a finger may miss a button.
///
/// 5 mm is the absolute minimum in Microsoft's touch guidance, which
/// recommends 7 to 9 mm on a phone. The recommended size cannot be the
/// threshold here: two columns give buttons 6.8 mm wide, so every user who
/// chose two columns would be warned for taking a choice the editor offers.
/// A warning on every use of a feature is noise, and noise hides the real
/// warnings. Not yet measured on the band.
pub const MIN_TOUCH_MM: f64 = 5.0;

use crate::layout::{Corners, Layout};
use serde::Serialize;

/// A button's place on the screen, in pixels. Signed, because a layout with
/// too many buttons gives a height below zero, and the checks report that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Rect {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

pub fn px_to_mm(px: f64) -> f64 {
    px / PPI * 25.4
}

/// Place every button, in order.
///
/// In one column, each button takes a row. In two columns, buttons fill a row
/// left then right, and a spanning button closes any half row and takes a row
/// to itself. Every row has the same height.
pub fn place(layout: &Layout) -> Vec<Rect> {
    let two_columns = layout.columns >= 2;
    // Each row lists (button index, full width).
    let mut rows: Vec<Vec<(usize, bool)>> = Vec::new();
    let mut half_row: Option<usize> = None;
    for (i, button) in layout.buttons.iter().enumerate() {
        if !two_columns || button.span {
            rows.push(vec![(i, true)]);
            half_row = None;
        } else if let Some(row) = half_row.take() {
            rows[row].push((i, false));
        } else {
            rows.push(vec![(i, false)]);
            half_row = Some(rows.len() - 1);
        }
    }

    let margin = MARGIN as i64;
    let area_h = SCREEN_H as i64 - margin - layout.bottom_strip as i64;
    let count = rows.len() as i64;
    let row_h = if count == 0 {
        0
    } else {
        (area_h - ROW_GAP as i64 * (count - 1)) / count
    };
    let full_w = SCREEN_W as i64 - 2 * margin;
    let half_w = (full_w - COL_GAP as i64) / 2;

    let mut rects = vec![Rect { x: 0, y: 0, w: 0, h: 0 }; layout.buttons.len()];
    for (r, row) in rows.iter().enumerate() {
        let y = margin + r as i64 * (row_h + ROW_GAP as i64);
        for (cell, (index, full)) in row.iter().enumerate() {
            rects[*index] = if *full {
                Rect { x: margin, y, w: full_w, h: row_h }
            } else {
                let x = margin + cell as i64 * (half_w + COL_GAP as i64);
                Rect { x, y, w: half_w, h: row_h }
            };
        }
    }
    rects
}

/// The corner radius of a button of this size, for the app's chosen shape.
///
/// A pill's radius is half its short side. A rounded corner is a fixed size,
/// so every button in an app has the same corner whatever its height, which is
/// what makes them look like one set.
pub fn corner_radius(corners: Corners, w: i64, h: i64) -> i64 {
    let half = w.min(h) / 2;
    match corners {
        Corners::Pill => half,
        Corners::Rounded => ROUNDED_RADIUS.min(half),
        Corners::Square => 0,
    }
}

/// How far a rounded rectangle's edge lies inside its box at this height.
///
/// Zero along the straight part, and infinite above the top or below the
/// bottom, where the shape draws nothing at all.
fn edge_inset(top: f64, bottom: f64, radius: f64, y: f64) -> f64 {
    if y < top || y > bottom {
        return f64::INFINITY; // Above or below the shape, which draws nothing.
    }
    if radius <= 0.0 {
        return 0.0; // A square corner: the edge is the box, all the way up.
    }
    let distance = if y < top + radius {
        top + radius - y
    } else if y > bottom - radius {
        y - (bottom - radius)
    } else {
        0.0
    };
    if distance >= radius {
        return f64::INFINITY;
    }
    radius - (radius * radius - distance * distance).sqrt()
}

/// Does this button reach outside the safe area?
///
/// A button is a rounded rectangle and the safe area is a capsule, so a short
/// button in the top or bottom row runs past the curve even though its box is
/// inside the screen. The generator draws such a button as an image, trimmed
/// to the curve. Every other button stays a plain widget, which needs no
/// image at all.
pub fn leaves_safe_area(rect: &Rect, radius: i64) -> bool {
    let radius = radius as f64;
    let (top, bottom) = (rect.y as f64, (rect.y + rect.h) as f64);
    let (safe_top, safe_bottom) = (MARGIN as f64, (SCREEN_H - MARGIN) as f64);
    // Half a pixel of overlap is not worth an image.
    const TOLERANCE: f64 = 0.5;
    (rect.y..rect.y + rect.h).any(|row| {
        let y = row as f64 + 0.5;
        let button = edge_inset(top, bottom, radius, y);
        if button.is_infinite() {
            return false; // The button draws nothing on this row.
        }
        let safe = edge_inset(safe_top, safe_bottom, SAFE_RADIUS as f64, y);
        if safe.is_infinite() {
            return true; // The safe area does not reach this row.
        }
        rect.x as f64 + button + TOLERANCE < MARGIN as f64 + safe
            || (rect.x + rect.w) as f64 - button - TOLERANCE > (SCREEN_W - MARGIN) as f64 - safe
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Button, Layout};

    fn layout(columns: u32, spans: &[bool], strip: u32) -> Layout {
        let mut l = Layout::new("T");
        l.columns = columns;
        l.bottom_strip = strip;
        for (i, span) in spans.iter().enumerate() {
            let mut b = Button::new(&format!("bb{:02x}", i + 4), "#1f6feb");
            b.span = *span;
            l.buttons.push(b);
        }
        l
    }

    #[test]
    fn two_buttons_in_one_column_match_band_arrows() {
        // band-app-arrows: BH = (490 - 6 - 70 - 10) / 2 = 202, second y = 6 + 202 + 10.
        let rects = place(&layout(1, &[false, false], 70));
        assert_eq!(rects[0], Rect { x: 6, y: 6, w: 180, h: 202 });
        assert_eq!(rects[1], Rect { x: 6, y: 218, w: 180, h: 202 });
    }

    #[test]
    fn two_columns_fill_left_then_right() {
        let rects = place(&layout(2, &[false, false, false], 70));
        assert_eq!((rects[0].x, rects[0].w), (6, 87));
        assert_eq!((rects[1].x, rects[1].w), (99, 87));
        assert_eq!(rects[0].y, rects[1].y);
        assert_eq!((rects[2].x, rects[2].w), (6, 87));
        assert!(rects[2].y > rects[0].y);
    }

    #[test]
    fn a_spanning_button_takes_a_whole_row() {
        let rects = place(&layout(2, &[false, true, false], 70));
        assert_eq!(rects[1].w, 180);
        assert!(rects[1].y > rects[0].y, "the half row closes before the span");
        assert!(rects[2].y > rects[1].y);
    }

    #[test]
    fn removing_the_strip_gives_the_buttons_the_room() {
        let rects = place(&layout(1, &[false], 0));
        assert_eq!(rects[0].h, 490 - 6);
    }

    #[test]
    fn too_many_buttons_leave_no_room() {
        let spans = vec![false; 60];
        let rects = place(&layout(1, &spans, 70));
        assert!(rects.iter().any(|r| r.h < 1));
    }

    /// A tall button follows the screen's curve by itself: with a radius of
    /// half its width, its arcs share the safe area's centers. A short button
    /// in the top row does not, so it needs an image trimmed to the curve.
    #[test]
    fn only_a_button_the_curve_reaches_leaves_the_safe_area() {
        let pill = |r: &Rect| leaves_safe_area(r, corner_radius(Corners::Pill, r.w, r.h));

        let one = place(&layout(1, &[false], 70));
        assert!(!pill(&one[0]), "{:?}", one[0]);

        let two = place(&layout(1, &[false; 2], 70));
        assert!(two.iter().all(|r| !pill(r)), "{two:?}");

        let four = place(&layout(1, &[false; 4], 70));
        assert!(pill(&four[0]), "the top row: {:?}", four[0]);
        assert!(!pill(&four[1]), "the second row: {:?}", four[1]);
        assert!(!pill(&four[2]), "the third row: {:?}", four[2]);

        let columns = place(&layout(2, &[false; 2], 70));
        assert!(columns.iter().all(pill), "{columns:?}");
    }

    /// A square corner reaches further out than a round one, so the curve
    /// catches a row the pill shape clears.
    #[test]
    fn square_corners_meet_the_curve_sooner() {
        let two = place(&layout(1, &[false; 2], 70));
        let top = &two[0];
        assert!(!leaves_safe_area(top, corner_radius(Corners::Pill, top.w, top.h)));
        assert!(leaves_safe_area(top, corner_radius(Corners::Square, top.w, top.h)));
    }

    #[test]
    fn each_shape_has_its_own_corner() {
        assert_eq!(corner_radius(Corners::Pill, 180, 96), 48);
        assert_eq!(corner_radius(Corners::Rounded, 180, 96), ROUNDED_RADIUS);
        assert_eq!(corner_radius(Corners::Square, 180, 96), 0);
        // A button shorter than the rounded corner keeps a shape it can draw.
        assert_eq!(corner_radius(Corners::Rounded, 180, 20), 10);
    }

    #[test]
    fn a_two_column_button_is_wider_than_the_threshold() {
        // 87 px is one button of two columns: 6.8 mm, above the 5 mm warning.
        assert!((px_to_mm(87.0) - 6.78).abs() < 0.01);
        assert!(px_to_mm(87.0) > MIN_TOUCH_MM);
        // 5 mm is 64.2 px, so the warning starts at 64 px.
        assert!(px_to_mm(65.0) > MIN_TOUCH_MM);
        assert!(px_to_mm(64.0) < MIN_TOUCH_MM);
    }
}
