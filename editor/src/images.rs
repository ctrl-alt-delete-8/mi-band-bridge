//! The PNG files the band shows: the app icon and button backgrounds.
//!
//! The band does not stretch an image, so each one is drawn at its final size.
//! zmake converts these PNG files to the TGA type the band reads.

use crate::geometry::{Rect, ICON, MARGIN, SAFE_RADIUS, SCREEN_H, SCREEN_W};
use std::path::Path as FilePath;
use tiny_skia::{
    Color, ColorU8, FillRule, FilterQuality, IntSize, Mask, Paint, Path, PathBuilder, Pixmap,
    PixmapPaint, Transform,
};

/// The picture formats the editor reads. zmake turns whatever the generator
/// writes into the band's own TGA type, so the format the user picked only has
/// to reach the generator.
pub const FORMATS: [&str; 3] = ["png", "jpg", "jpeg"];

/// Read a PNG or JPEG file.
///
/// The format comes from the file's own first bytes, not from its name, so a
/// JPEG saved as `.png` still opens.
pub fn load(path: &FilePath) -> Result<Pixmap, String> {
    let reader = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let rgba = reader.decode().map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut data = rgba.into_raw();
    // tiny-skia holds color multiplied by alpha; the image crate does not.
    for pixel in data.chunks_exact_mut(4) {
        let alpha = pixel[3] as u32;
        for channel in &mut pixel[..3] {
            *channel = (*channel as u32 * alpha / 255) as u8;
        }
    }
    let size = IntSize::from_wh(w, h).ok_or("the image has no size")?;
    Pixmap::from_vec(data, size).ok_or_else(|| "the image is too large".to_string())
}

fn paint(rgb: (u8, u8, u8)) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(rgb.0, rgb.1, rgb.2, 255));
    paint.anti_alias = true;
    paint
}

/// A rectangle with round corners. A radius of half the short side gives a pill.
fn rounded_rect(w: f32, h: f32, radius: f32) -> Option<Path> {
    let r = radius.min(w / 2.0).min(h / 2.0);
    let mut pb = PathBuilder::new();
    pb.move_to(r, 0.0);
    pb.line_to(w - r, 0.0);
    pb.quad_to(w, 0.0, w, r);
    pb.line_to(w, h - r);
    pb.quad_to(w, h, w - r, h);
    pb.line_to(r, h);
    pb.quad_to(0.0, h, 0.0, h - r);
    pb.line_to(0.0, r);
    pb.quad_to(0.0, 0.0, r, 0.0);
    pb.close();
    pb.finish()
}

/// The safe area, in one button's own coordinates.
///
/// The screen is a capsule, so a button in the top or bottom row runs past the
/// curve. Drawing through this mask trims it there, and the gap to the glass
/// stays MARGIN the whole way round, like the sides.
fn safe_mask(rect: &Rect) -> Option<Mask> {
    let mut mask = Mask::new(rect.w.max(1) as u32, rect.h.max(1) as u32)?;
    let safe = rounded_rect(
        (SCREEN_W - 2 * MARGIN) as f32,
        (SCREEN_H - 2 * MARGIN) as f32,
        SAFE_RADIUS as f32,
    )?;
    // The shape is in screen coordinates. Move it into the button's.
    mask.fill_path(
        &safe,
        FillRule::Winding,
        true,
        Transform::from_translate(MARGIN as f32 - rect.x as f32, MARGIN as f32 - rect.y as f32),
    );
    Some(mask)
}

/// Draw `source` scaled to fit a box centered on (cx, cy), keeping its shape.
fn draw_fitted(
    target: &mut Pixmap,
    source: &Pixmap,
    cx: f32,
    cy: f32,
    box_w: f32,
    box_h: f32,
    mask: Option<&Mask>,
) {
    let (sw, sh) = (source.width() as f32, source.height() as f32);
    let scale = (box_w / sw).min(box_h / sh);
    let (dw, dh) = (sw * scale, sh * scale);
    let transform = Transform::from_row(scale, 0.0, 0.0, scale, cx - dw / 2.0, cy - dh / 2.0);
    let quality = PixmapPaint {
        quality: FilterQuality::Bicubic,
        ..PixmapPaint::default()
    };
    target.draw_pixmap(0, 0, source.as_ref(), &quality, transform, mask);
}

/// The 100 × 100 app icon. The user's PNG scaled to fit, or a plain disc in
/// the app color when there is none.
pub fn app_icon(source: Option<&Pixmap>, rgb: (u8, u8, u8)) -> Pixmap {
    let mut icon = Pixmap::new(ICON, ICON).expect("100 × 100 is a valid size");
    let half = ICON as f32 / 2.0;
    match source {
        Some(image) => draw_fitted(&mut icon, image, half, half, ICON as f32, ICON as f32, None),
        None => {
            // 2 px clear at the edge, like the existing apps' icons.
            if let Some(disc) = PathBuilder::from_circle(half, half, half - 2.0) {
                icon.fill_path(&disc, &paint(rgb), FillRule::Winding, Transform::identity(), None);
            }
        }
    }
    icon
}

/// The user's picture alone, scaled into a square for the form.
///
/// The form shows the picture; the preview beside it shows the button, so the
/// row does not repeat the color, the shape, or the label.
pub fn thumbnail(source: &Pixmap, size: u32) -> Pixmap {
    let mut out = Pixmap::new(size.max(1), size.max(1)).expect("a thumbnail is at least 1 px");
    let half = size as f32 / 2.0;
    draw_fitted(&mut out, source, half, half, size as f32, size as f32, None);
    out
}

/// The picture as a press shows it: each pixel half way to the pressed
/// color, with its own alpha. The margin around it takes the pressed color
/// whole, so the picture changes with it.
pub fn pressed_picture(picture: &Pixmap, (r, g, b): (u8, u8, u8)) -> Pixmap {
    let half = |from: u8, to: u8| ((u16::from(from) + u16::from(to)) / 2) as u8;
    let mut out = picture.clone();
    for pixel in out.pixels_mut() {
        let c = pixel.demultiply();
        *pixel = ColorU8::from_rgba(
            half(c.red(), r),
            half(c.green(), g),
            half(c.blue(), b),
            c.alpha(),
        )
        .premultiply();
    }
    out
}

/// A button background: the button's shape in its color, trimmed to the safe
/// area, with the user's picture filling it.
pub fn button_image(
    rect: &Rect,
    radius: i64,
    rgb: (u8, u8, u8),
    source: Option<&Pixmap>,
) -> Pixmap {
    let (w, h) = (rect.w.max(1) as u32, rect.h.max(1) as u32);
    let mut image = Pixmap::new(w, h).expect("a button is at least 1 px");
    let (wf, hf) = (w as f32, h as f32);
    let pill = rounded_rect(wf, hf, radius as f32);
    let mut mask = safe_mask(rect);
    if let Some(shape) = &pill {
        image.fill_path(shape, &paint(rgb), FillRule::Winding, Transform::identity(), mask.as_ref());
    }
    if let Some(source) = source {
        // The picture is cut to the button's own shape as well, so filling the
        // button cannot put a square corner outside the pill.
        if let (Some(mask), Some(shape)) = (mask.as_mut(), &pill) {
            mask.intersect_path(shape, FillRule::Winding, true, Transform::identity());
        }
        // The picture fills the button, less a margin so it does not touch the
        // edge. It never shares the button with a label: see the generator.
        let inset = 2.0 * MARGIN as f32;
        draw_fitted(&mut image, source, wf / 2.0, hf / 2.0, wf - inset, hf - inset, mask.as_ref());
    }
    image
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A small JPEG, for the tests that need one on disk.
    pub(crate) fn jpeg_bytes() -> Vec<u8> {
        let source = image::RgbImage::from_pixel(8, 8, image::Rgb([255, 0, 0]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(source)
            .write_to(&mut out, image::ImageFormat::Jpeg)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn a_jpeg_reads_back_as_its_own_color() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("red.jpg");
        std::fs::write(&path, jpeg_bytes()).unwrap();
        let pixmap = load(&path).unwrap();
        assert_eq!((pixmap.width(), pixmap.height()), (8, 8));
        let (r, g, b, a) = rgb_at(&pixmap, 4, 4);
        assert_eq!(a, 255);
        assert!(r > 240 && g < 15 && b < 15, "got {r},{g},{b}");
    }

    fn solid(w: u32, h: u32, rgba: (u8, u8, u8, u8)) -> Pixmap {
        let mut p = Pixmap::new(w, h).unwrap();
        p.fill(Color::from_rgba8(rgba.0, rgba.1, rgba.2, rgba.3));
        p
    }

    fn rgb_at(p: &Pixmap, x: u32, y: u32) -> (u8, u8, u8, u8) {
        let c = p.pixel(x, y).unwrap();
        (c.red(), c.green(), c.blue(), c.alpha())
    }

    #[test]
    fn an_icon_without_a_source_is_a_disc_in_the_app_color() {
        let icon = app_icon(None, (31, 111, 235));
        assert_eq!((icon.width(), icon.height()), (100, 100));
        assert_eq!(rgb_at(&icon, 50, 50), (31, 111, 235, 255));
        assert_eq!(rgb_at(&icon, 0, 0).3, 0, "the corner is transparent");
    }

    #[test]
    fn an_icon_source_is_scaled_to_fill_the_icon() {
        let icon = app_icon(Some(&solid(10, 10, (255, 0, 0, 255))), (0, 0, 255));
        assert_eq!((icon.width(), icon.height()), (100, 100));
        assert_eq!(rgb_at(&icon, 50, 50), (255, 0, 0, 255));
    }

    /// The top button of a two button app: it fills the safe area exactly.
    fn tall_button() -> Rect {
        Rect { x: 6, y: 6, w: 180, h: 202 }
    }

    /// The pill radius for a rectangle of this size.
    fn pill(rect: &Rect) -> i64 {
        rect.w.min(rect.h) / 2
    }

    #[test]
    fn a_button_image_is_exactly_the_button_size() {
        let image = button_image(&tall_button(), pill(&tall_button()), (31, 111, 235), None);
        assert_eq!((image.width(), image.height()), (180, 202));
        assert_eq!(rgb_at(&image, 90, 101), (31, 111, 235, 255), "the center is the button color");
        assert_eq!(rgb_at(&image, 0, 0).3, 0, "the rounded corner is transparent");
    }

    /// Without a label the picture fills the button, less a small margin, so
    /// it is not a stamp in the middle of a colored pill.
    #[test]
    fn without_a_label_the_image_fills_the_button() {
        let red = solid(10, 10, (255, 0, 0, 255));
        let image = button_image(&tall_button(), pill(&tall_button()), (0, 0, 255), Some(&red));
        assert_eq!(rgb_at(&image, 90, 20), (255, 0, 0, 255), "near the top edge");
        assert_eq!(rgb_at(&image, 90, 182), (255, 0, 0, 255), "near the bottom edge");
        assert_eq!(rgb_at(&image, 90, 101), (255, 0, 0, 255), "and the middle");
    }

    /// Square corners reach the box's own corner, where a pill is empty.
    #[test]
    fn a_square_button_fills_its_corner() {
        let rect = Rect { x: 6, y: 218, w: 180, h: 96 };
        let square = button_image(&rect, 0, (31, 111, 235), None);
        assert_eq!(rgb_at(&square, 0, 0), (31, 111, 235, 255), "the square corner is filled");
        let round = button_image(&rect, pill(&rect), (31, 111, 235), None);
        assert_eq!(rgb_at(&round, 0, 0).3, 0, "the pill corner is not");
    }

    /// A press turns the picture half way to the pressed color. Each pixel
    /// keeps its alpha, so a clear pixel stays clear.
    #[test]
    fn a_pressed_picture_turns_half_way_to_the_pressed_color() {
        let mut picture = Pixmap::new(3, 1).unwrap();
        picture.pixels_mut()[0] = ColorU8::from_rgba(255, 0, 0, 255).premultiply();
        picture.pixels_mut()[1] = ColorU8::from_rgba(255, 0, 0, 128).premultiply();
        let pressed = pressed_picture(&picture, (0, 0, 255));
        let at = |x| {
            let c = pressed.pixel(x, 0).unwrap().demultiply();
            (c.red(), c.green(), c.blue(), c.alpha())
        };
        assert_eq!(at(0), (127, 0, 127, 255));
        let (r, g, b, a) = at(1);
        assert_eq!(a, 128, "the alpha stays");
        assert!(
            r.abs_diff(127) <= 1 && g == 0 && b.abs_diff(127) <= 1,
            "{r},{g},{b}"
        );
        assert_eq!(at(2).3, 0, "a clear pixel stays clear");
        assert_eq!((pressed.width(), pressed.height()), (3, 1));
    }

    /// Two buttons of one size, one in the top row and one in the middle. The
    /// same point is on the screen in the middle of the screen and off it at
    /// the top, so the top one loses it to the curve.
    #[test]
    fn the_top_row_is_trimmed_to_the_screen_curve() {
        const TOP_ROW: Rect = Rect { x: 6, y: 6, w: 180, h: 96 };
        const MIDDLE_ROW: Rect = Rect { x: 6, y: 218, w: 180, h: 96 };
        let color = (31, 111, 235);
        let top = button_image(&TOP_ROW, pill(&TOP_ROW), color, None);
        let middle = button_image(&MIDDLE_ROW, pill(&MIDDLE_ROW), color, None);

        // (20, 14) is inside a button of this size, and outside the safe area
        // when the button sits at the top of the screen.
        assert_eq!(rgb_at(&top, 20, 14).3, 0, "the curve trims the top corner");
        assert_eq!(rgb_at(&middle, 20, 14), (31, 111, 235, 255), "the middle row keeps it");
        assert_eq!(rgb_at(&top, 90, 48), (31, 111, 235, 255), "the middle of the button stays");
        assert_eq!(rgb_at(&top, 159, 14).3, 0, "and the other corner too");
    }
}
