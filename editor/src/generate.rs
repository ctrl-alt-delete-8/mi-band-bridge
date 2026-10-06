//! Write a zmake project from a layout.
//!
//! Everything under `generated/` is made from `layout.json` and written again
//! on every run, so a stale file can never survive a change.

use crate::geometry::{self, Rect};
use crate::images;
use crate::label;
use crate::layout::{self, Button, Layout};
use std::fs;
use std::path::{Path, PathBuf};
use tiny_skia::Pixmap;

const PAGE_TEMPLATE: &str = include_str!("../template/page.js");
const APP_JS: &str = include_str!("../template/app.js");
const NOTICE: &str = "The Mi Band App Editor writes this folder from layout.json on every build.\nDo not edit it. Change the app in the editor instead.\n";

pub struct Generated {
    pub project: PathBuf,
    pub page: String,
    pub assets: Vec<(String, PathBuf)>,
}

#[derive(Debug, thiserror::Error)]
pub enum GenerateError {
    #[error("cannot write {path}: {reason}")]
    Write { path: String, reason: String },
    #[error("button {0} has no room on the screen")]
    NoRoom(usize),
    #[error("button {0} has an invalid signal")]
    BadSignal(usize),
    #[error("\"{0}\" is not a color. Use #rrggbb.")]
    BadColor(String),
    #[error("cannot use image {path}: {reason}")]
    Image { path: String, reason: String },
}

fn write_err(path: &Path) -> impl Fn(std::io::Error) -> GenerateError + '_ {
    move |e| GenerateError::Write { path: path.display().to_string(), reason: e.to_string() }
}

fn save_png(image: &Pixmap, path: &Path) -> Result<(), GenerateError> {
    image.save_png(path).map_err(|e| GenerateError::Write {
        path: path.display().to_string(),
        reason: e.to_string(),
    })
}

fn load_image(app_dir: &Path, rel: &str) -> Result<Pixmap, GenerateError> {
    let path = layout::image_path(app_dir, rel).ok_or_else(|| GenerateError::Image {
        path: rel.to_string(),
        reason: "it must be inside the app folder".to_string(),
    })?;
    images::load(&path).map_err(|reason| GenerateError::Image {
        path: rel.to_string(),
        reason,
    })
}

fn color(text: &str) -> Result<(u8, u8, u8), GenerateError> {
    layout::parse_color(text).ok_or_else(|| GenerateError::BadColor(text.to_string()))
}

/// A JavaScript string literal. JSON strings are valid ES5, except for two
/// line separators that JSON allows raw and ES5 does not.
fn js_string(text: &str) -> String {
    serde_json::to_string(text)
        .expect("a string always serialises")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn js_rgb((r, g, b): (u8, u8, u8)) -> String {
    format!("0x{r:02x}{g:02x}{b:02x}")
}

fn app_json(layout: &Layout) -> String {
    let value = serde_json::json!({
        "app": {
            "appId": layout.app_id,
            "appIdType": 0,
            "appName": layout.name,
            "appType": "app",
            "description": "Made with the Mi Band App Editor.",
            "icon": "icon.png",
            "vender": "bandbridge",
            "version": { "code": layout.version, "name": format!("1.0.{}", layout.version) }
        },
        "configVersion": "v2",
        "debug": false,
        "i18n": {},
        "module": {
            "page": {
                "pages": ["page/index"],
                "window": {
                    "backgroundColor": "#000000",
                    "backgroundTextStyle": "light",
                    "navigationBarBackgroundColor": "#000000",
                    "navigationBarTextStyle": "white",
                    "navigationBarTitleText": ""
                }
            }
        },
        "permissions": [],
        "runtime": {
            "apiVersion": { "compatible": "1.0.0", "minVersion": "1.0.0", "target": "1.0.1" }
        }
    });
    serde_json::to_string_pretty(&value).expect("json always serialises") + "\n"
}

#[allow(clippy::too_many_arguments)]
fn entry(
    button: &Button,
    rect: &Rect,
    radius: i64,
    size: u32,
    rgb: (u8, u8, u8),
    press: (u8, u8, u8),
    bytes: &[u8],
    image: Option<&str>,
    press_image: Option<&str>,
) -> String {
    let bytes = bytes.iter().map(|b| format!("0x{b:02x}")).collect::<Vec<_>>().join(", ");
    let name = |n: Option<&str>| n.map(js_string).unwrap_or_else(|| "null".to_string());
    // A picture takes the whole button, so the label is not drawn. The band
    // draws a label across the middle of the widget and cannot move it, so
    // showing both means shrinking both. The label stays in layout.json and
    // comes back when the picture goes. The band draws each "\n" of a label
    // as a new line, so the breaks are written into the label.
    let text = if button.image.is_some() {
        String::new()
    } else {
        label::for_band(&button.label, size, rect.w, rect.h)
    };
    format!(
        "  {{ x: {}, y: {}, w: {}, h: {}, radius: {}, text: {}, textSize: {}, color: {}, pressColor: {}, image: {}, pressImage: {}, bytes: [{}] }}",
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        radius,
        js_string(&text),
        size,
        js_rgb(rgb),
        js_rgb(press),
        name(image),
        name(press_image),
        bytes
    )
}

pub fn generate(app_dir: &Path, layout: &Layout) -> Result<Generated, GenerateError> {
    let name = app_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "app".to_string());
    let root = app_dir.join("generated");
    if root.exists() {
        fs::remove_dir_all(&root).map_err(write_err(&root))?;
    }
    let project = root.join(&name);
    let assets_dir = project.join("assets");
    fs::create_dir_all(project.join("page")).map_err(write_err(&project))?;
    fs::create_dir_all(&assets_dir).map_err(write_err(&assets_dir))?;

    let notice = root.join("README.txt");
    fs::write(&notice, NOTICE).map_err(write_err(&notice))?;
    let manifest = project.join("app.json");
    fs::write(&manifest, app_json(layout)).map_err(write_err(&manifest))?;
    let app_js = project.join("app.js");
    fs::write(&app_js, APP_JS).map_err(write_err(&app_js))?;

    let app_rgb = color(&layout.color)?;
    let icon_source = match &layout.icon {
        Some(rel) => Some(load_image(app_dir, rel)?),
        None => None,
    };
    let icon_path = assets_dir.join("icon.png");
    save_png(&images::app_icon(icon_source.as_ref(), app_rgb), &icon_path)?;
    let mut assets = vec![("icon.png".to_string(), icon_path)];

    let rects = geometry::place(layout);
    let size = label::app_size(layout, &rects).size;
    let mut entries = Vec::new();
    for (i, (button, rect)) in layout.buttons.iter().zip(&rects).enumerate() {
        if rect.w < 1 || rect.h < 1 {
            return Err(GenerateError::NoRoom(i + 1));
        }
        let bytes = layout::parse_signal(&button.signal).ok_or(GenerateError::BadSignal(i + 1))?;
        let rgb = color(&button.color)?;
        let press = match &button.press_color {
            Some(c) => color(c)?,
            None => layout::lighter(rgb),
        };
        let source = match &button.image {
            Some(rel) => Some(load_image(app_dir, rel)?),
            None => None,
        };
        // A plain widget draws a rectangle with round corners, which runs past
        // the screen's curve in the top and bottom rows. An image can be
        // trimmed to that curve, so draw one whenever the widget would not do:
        // for the user's own picture, or for a button the curve reaches.
        let radius = geometry::corner_radius(layout.corners, rect.w, rect.h);
        let (image, press_image) = if source.is_some() || geometry::leaves_safe_area(rect, radius) {
            let normal = format!("button_{i}.png");
            let pressed = format!("button_{i}_press.png");
            let normal_path = assets_dir.join(&normal);
            let pressed_path = assets_dir.join(&pressed);
            let art =
                |fill, picture: Option<&Pixmap>| images::button_image(rect, radius, fill, picture);
            save_png(&art(rgb, source.as_ref()), &normal_path)?;
            // A press takes the margin to the pressed color, and the picture
            // half way to it, so the whole button answers a press.
            let picture_pressed = source
                .as_ref()
                .map(|picture| images::pressed_picture(picture, press));
            save_png(&art(press, picture_pressed.as_ref()), &pressed_path)?;
            assets.push((normal.clone(), normal_path));
            assets.push((pressed.clone(), pressed_path));
            (Some(normal), Some(pressed))
        } else {
            (None, None)
        };
        entries.push(entry(
            button,
            rect,
            radius,
            size,
            rgb,
            press,
            &bytes,
            image.as_deref(),
            press_image.as_deref(),
        ));
    }

    let page = PAGE_TEMPLATE.replace("/*@BUTTONS@*/", &entries.join(",\n"));
    let page_path = project.join("page").join("index.js");
    fs::write(&page_path, &page).map_err(write_err(&page_path))?;
    Ok(Generated { project, page, assets })
}

/// Copy a PNG the user picked into the app's `images/` folder.
pub fn copy_image(app_dir: &Path, source: &Path) -> std::io::Result<String> {
    let stem = source
        .file_stem()
        .map(|s| layout::slug(&s.to_string_lossy()))
        .unwrap_or_else(|| "image".to_string());
    // Keep the file's own type. The generator reads PNG and JPEG alike.
    let kind = source
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .filter(|e| images::FORMATS.contains(&e.as_str()))
        .unwrap_or_else(|| "png".to_string());
    let folder = app_dir.join("images");
    fs::create_dir_all(&folder)?;
    let mut name = format!("{stem}.{kind}");
    let mut n = 2;
    while folder.join(&name).exists() {
        name = format!("{stem}-{n}.{kind}");
        n += 1;
    }
    fs::copy(source, folder.join(&name))?;
    Ok(format!("images/{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Button;

    fn app_dir() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("slides");
        std::fs::create_dir_all(dir.join("images")).unwrap();
        (root, dir)
    }

    fn two_buttons() -> Layout {
        let mut l = Layout::new("Slides");
        let mut back = Button::new("bb04", "#1f6feb");
        back.label = "Back".to_string();
        l.buttons.push(back);
        l.buttons.push(Button::new("BB05", "#1f6feb"));
        l
    }

    fn red_png(path: &Path) {
        let mut p = tiny_skia::Pixmap::new(8, 8).unwrap();
        p.fill(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        p.save_png(path).unwrap();
    }

    #[test]
    fn it_writes_a_zmake_project_named_after_the_app_folder() {
        let (_root, dir) = app_dir();
        let made = generate(&dir, &two_buttons()).unwrap();
        assert_eq!(made.project, dir.join("generated").join("slides"));
        for file in ["app.json", "app.js", "page/index.js", "assets/icon.png"] {
            assert!(made.project.join(file).is_file(), "{file}");
        }
        assert!(dir.join("generated/README.txt").is_file());
    }

    #[test]
    fn app_json_carries_the_layout_values_and_no_personal_name() {
        let (_root, dir) = app_dir();
        let layout = two_buttons();
        let made = generate(&dir, &layout).unwrap();
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(made.project.join("app.json")).unwrap()).unwrap();
        assert_eq!(json["app"]["appId"], layout.app_id);
        assert_eq!(json["app"]["appName"], "Slides");
        assert_eq!(json["app"]["vender"], "bandbridge");
        assert_eq!(json["app"]["version"]["code"], layout.version);
    }

    #[test]
    fn app_js_is_the_proven_file() {
        let (_root, dir) = app_dir();
        let made = generate(&dir, &two_buttons()).unwrap();
        let expected = include_str!("../../band-app-arrows/app.js");
        assert_eq!(std::fs::read_to_string(made.project.join("app.js")).unwrap(), expected);
    }

    #[test]
    fn the_page_holds_each_button_with_its_bytes() {
        let (_root, dir) = app_dir();
        let made = generate(&dir, &two_buttons()).unwrap();
        assert!(made.page.contains("bytes: [0xbb, 0x04]"), "{}", made.page);
        assert!(made.page.contains("bytes: [0xbb, 0x05]"), "{}", made.page);
        assert!(made.page.contains("text: \"Back\""));
        assert!(made.page.contains("x: 6, y: 6, w: 180, h: 202"));
        assert!(!made.page.contains("/*@BUTTONS@*/"));
    }

    /// Every labeled button gets the app's one size, and each label its line
    /// breaks. Two columns give buttons 87 px wide, where "Next slide" takes
    /// two lines. "Previous" does not fit even at 25 px, so with it the app
    /// takes 25 px, and "Previous" stays on one line.
    #[test]
    fn every_label_gets_the_app_s_one_size_and_its_line_breaks() {
        let (_root, dir) = app_dir();
        let mut layout = two_buttons();
        layout.columns = 2;
        layout.buttons[0].label = "Next slide".to_string();
        layout.buttons[1].label = "Back".to_string();
        let made = generate(&dir, &layout).unwrap();
        assert!(
            made.page
                .contains(r#"w: 87, h: 414, radius: 43, text: "Next\nslide", textSize: 34,"#),
            "{}",
            made.page
        );
        assert!(
            made.page.contains(r#"text: "Back", textSize: 34,"#),
            "{}",
            made.page
        );

        layout.buttons[1].label = "Previous".to_string();
        let made = generate(&dir, &layout).unwrap();
        assert!(
            made.page.contains(r#"text: "Next\nslide", textSize: 25,"#),
            "{}",
            made.page
        );
        assert!(
            made.page.contains(r#"text: "Previous", textSize: 25,"#),
            "{}",
            made.page
        );

        // With smaller text allowed, the app takes 20 px, where "Previous" fits.
        layout.small_text = true;
        let made = generate(&dir, &layout).unwrap();
        assert!(
            made.page.contains(r#"text: "Next\nslide", textSize: 20,"#),
            "{}",
            made.page
        );
        assert!(
            made.page.contains(r#"text: "Previous", textSize: 20,"#),
            "{}",
            made.page
        );

        // A picture hides the label, so there is no text to fit.
        red_png(&dir.join("images/next.png"));
        layout.buttons[1].image = Some("images/next.png".to_string());
        let made = generate(&dir, &layout).unwrap();
        assert!(
            made.page.contains(r#"text: "", textSize: 34,"#),
            "{}",
            made.page
        );
        assert!(
            made.page.contains(r#"text: "Next\nslide", textSize: 34,"#),
            "{}",
            made.page
        );
    }

    #[test]
    fn the_page_uses_no_syntax_the_band_rejects() {
        let (_root, dir) = app_dir();
        let made = generate(&dir, &two_buttons()).unwrap();
        for bad in ["=>", "let ", "const ", "`", "class "] {
            assert!(!made.page.contains(bad), "the page contains {bad:?}");
        }
    }

    #[test]
    fn a_quote_in_a_label_is_escaped() {
        let (_root, dir) = app_dir();
        let mut layout = two_buttons();
        layout.buttons[0].label = "Say \"hi\"".to_string();
        let made = generate(&dir, &layout).unwrap();
        assert!(made.page.contains(r#"text: "Say \"hi\"""#), "{}", made.page);
    }

    #[test]
    fn an_image_button_gets_both_pngs_at_its_size() {
        let (_root, dir) = app_dir();
        red_png(&dir.join("images/next.png"));
        let mut layout = two_buttons();
        layout.buttons[1].image = Some("images/next.png".to_string());
        let made = generate(&dir, &layout).unwrap();
        for name in ["button_1.png", "button_1_press.png"] {
            let png = tiny_skia::Pixmap::load_png(made.project.join("assets").join(name)).unwrap();
            assert_eq!((png.width(), png.height()), (180, 202), "{name}");
            assert!(made.assets.iter().any(|(n, _)| n == name));
        }
        assert!(made.page.contains("image: \"button_1.png\""));
        assert!(made.page.contains("image: null"), "the other button has no image");
    }

    /// A press changes the picture with its margin. The pressed image has the
    /// margin in the pressed color and the picture half way to it, so with
    /// the automatic pressed color, half way to white, both turn lighter.
    #[test]
    fn a_pressed_picture_changes_with_its_margin() {
        let (_root, dir) = app_dir();
        red_png(&dir.join("images/next.png"));
        let mut layout = two_buttons();
        layout.buttons[1].image = Some("images/next.png".to_string());
        let made = generate(&dir, &layout).unwrap();
        let rgba = |name: &str, x, y| {
            let png = tiny_skia::Pixmap::load_png(made.project.join("assets").join(name)).unwrap();
            let c = png.pixel(x, y).unwrap().demultiply();
            (c.red(), c.green(), c.blue(), c.alpha())
        };
        let close = |got: (u8, u8, u8, u8), want: (u8, u8, u8, u8)| {
            got.0.abs_diff(want.0) <= 1
                && got.1.abs_diff(want.1) <= 1
                && got.2.abs_diff(want.2) <= 1
                && got.3 == want.3
        };
        // The button is #1f6feb, and its automatic pressed color #8fb7f5.
        // (90, 5) is in the margin, above the picture, and (90, 101) in the
        // middle of the picture.
        assert_eq!(rgba("button_1.png", 90, 5), (31, 111, 235, 255));
        assert_eq!(rgba("button_1.png", 90, 101), (255, 0, 0, 255));
        assert_eq!(rgba("button_1_press.png", 90, 5), (143, 183, 245, 255));
        let middle = rgba("button_1_press.png", 90, 101);
        assert!(close(middle, (199, 91, 122, 255)), "{middle:?}");

        // A pressed color of its own sets both as well.
        layout.buttons[1].press_color = Some("#000000".to_string());
        let made = generate(&dir, &layout).unwrap();
        let png =
            tiny_skia::Pixmap::load_png(made.project.join("assets/button_1_press.png")).unwrap();
        let at = |x, y| {
            let c = png.pixel(x, y).unwrap().demultiply();
            (c.red(), c.green(), c.blue(), c.alpha())
        };
        assert_eq!(at(90, 5), (0, 0, 0, 255));
        assert!(close(at(90, 101), (127, 0, 0, 255)), "{:?}", at(90, 101));
    }

    /// A picture takes the whole button, so the label is not drawn. It stays
    /// in the file, and comes back when the picture goes.
    #[test]
    fn a_picture_replaces_the_label() {
        let (_root, dir) = app_dir();
        red_png(&dir.join("images/next.png"));
        let mut layout = two_buttons();
        layout.buttons[0].label = "Back".to_string();
        layout.buttons[1].label = "Next".to_string();
        layout.buttons[1].image = Some("images/next.png".to_string());
        let made = generate(&dir, &layout).unwrap();
        assert!(made.page.contains(r#"text: "Back""#), "{}", made.page);
        assert!(!made.page.contains(r#"text: "Next""#), "{}", made.page);

        layout.buttons[1].image = None;
        let made = generate(&dir, &layout).unwrap();
        assert!(made.page.contains(r#"text: "Next""#), "the label comes back");
    }

    /// Every button in an app has the same corner, from one app-wide choice.
    #[test]
    fn the_app_shape_sets_every_corner() {
        let (_root, dir) = app_dir();
        let mut layout = two_buttons();
        layout.corners = crate::layout::Corners::Square;
        let made = generate(&dir, &layout).unwrap();
        assert_eq!(made.page.matches("radius: 0,").count(), 2, "{}", made.page);

        layout.corners = crate::layout::Corners::Rounded;
        let made = generate(&dir, &layout).unwrap();
        assert_eq!(made.page.matches("radius: 16,").count(), 2, "{}", made.page);
    }

    /// A plain widget cannot follow the screen's curve, so a button the curve
    /// reaches is drawn as an image even when the user gave no picture. Every
    /// other button stays a widget, and the app carries no image at all.
    #[test]
    fn only_a_button_the_curve_reaches_gets_art_of_its_own() {
        let (_root, dir) = app_dir();
        let mut layout = Layout::new("Rows");
        for i in 0..4 {
            layout.buttons.push(Button::new(&format!("bb{:02x}", i + 4), "#1f6feb"));
        }
        let made = generate(&dir, &layout).unwrap();
        assert!(made.page.contains("image: \"button_0.png\""), "the top row: {}", made.page);
        assert!(made.project.join("assets/button_0_press.png").is_file());
        assert!(!made.project.join("assets/button_1.png").exists(), "a middle row needs none");
        // The free strip holds the bottom row clear of the lower curve.
        assert!(!made.project.join("assets/button_3.png").exists(), "the bottom row is clear");

        // Without the strip, the bottom row reaches the curve and needs art.
        layout.bottom_strip = 0;
        let made = generate(&dir, &layout).unwrap();
        assert!(made.project.join("assets/button_3.png").is_file());

        let plain = generate(&dir, &two_buttons()).unwrap();
        assert!(!plain.page.contains("button_0.png"), "{}", plain.page);
        assert_eq!(plain.assets.len(), 1, "only the app icon: {:?}", plain.assets);
    }

    #[test]
    fn a_second_generate_removes_stale_files() {
        let (_root, dir) = app_dir();
        red_png(&dir.join("images/next.png"));
        let mut layout = two_buttons();
        layout.buttons[1].image = Some("images/next.png".to_string());
        let made = generate(&dir, &layout).unwrap();
        assert!(made.project.join("assets/button_1.png").is_file());
        layout.buttons[1].image = None;
        let made = generate(&dir, &layout).unwrap();
        assert!(!made.project.join("assets/button_1.png").exists());
    }

    #[test]
    fn a_layout_with_no_room_is_refused() {
        let (_root, dir) = app_dir();
        let mut layout = two_buttons();
        layout.bottom_strip = 490;
        assert!(generate(&dir, &layout).is_err());
    }

    #[test]
    fn every_layout_the_checks_accept_can_be_generated() {
        let (_root, dir) = app_dir();
        red_png(&dir.join("images/next.png"));

        let mut one_button = Layout::new("One");
        one_button.buttons.push(Button::new("bb04", "#1f6feb"));

        let two = two_buttons();

        let mut spanning = two_buttons();
        spanning.columns = 2;
        spanning.buttons[0].span = true;

        let mut no_strip = two_buttons();
        no_strip.bottom_strip = 0;

        let mut with_image = two_buttons();
        with_image.buttons[1].image = Some("images/next.png".to_string());
        with_image.buttons[1].label = "Next".to_string();

        let mut with_icon = two_buttons();
        with_icon.icon = Some("images/next.png".to_string());

        for layout in [one_button, two, spanning, no_strip, with_image, with_icon] {
            let report = crate::checks::check(&layout, &dir, &[]);
            assert!(report.can_build(), "{}: {:?}", layout.name, report.errors);
            assert!(generate(&dir, &layout).is_ok(), "{}", layout.name);
        }
    }

    #[test]
    fn a_copied_image_gets_a_unique_name_in_images() {
        let (root, dir) = app_dir();
        let source = root.path().join("My Arrow.png");
        red_png(&source);
        assert_eq!(copy_image(&dir, &source).unwrap(), "images/my-arrow.png");
        assert_eq!(copy_image(&dir, &source).unwrap(), "images/my-arrow-2.png");
        assert!(dir.join("images/my-arrow-2.png").is_file());
    }
}
