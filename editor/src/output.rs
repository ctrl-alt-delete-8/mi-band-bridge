//! What a build and an install say in the Output drawer, line by line, as
//! each step happens (decision E4).
//!
//! Each line carries the moment it was said, and a tone that says what kind
//! of line it is. The words live here, beside the steps they describe, as the
//! checks' words live in `checks.rs`.

use crate::install::{Heard, Stop};
use crate::layout::{self, Layout};
use crate::{checks, generate, signals, zmake};
use serde::Serialize;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// One line of the Output drawer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Said {
    /// When it was said, in milliseconds since 1970. The window writes it in
    /// the system's time zone.
    pub at: u64,
    pub text: String,
    pub tone: Tone,
    /// The upload's progress, from 0 to 100. A line with it takes the place
    /// of the last line with it, so the upload keeps to one line.
    pub percent: Option<u8>,
}

/// What kind of line it is, which sets its color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    /// What the editor, zmake or BandBridge reports.
    Note,
    /// A step the editor starts: the zmake command, and the install.
    Step,
    /// What a build or an install made.
    Good,
    /// The upload to the band, as it goes.
    Progress,
    /// Why a build or an install stopped.
    Bad,
}

impl Said {
    /// A line said now.
    pub fn now(tone: Tone, text: impl Into<String>) -> Said {
        Said {
            at: now(),
            text: text.into(),
            tone,
            percent: None,
        }
    }
}

/// Now, in milliseconds since 1970.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or_default()
}

/// What the log says when an install stops because BandBridge is not
/// running, or its band is not connected. The window's card for each says
/// which, and what to do, so the log does not say it again. BandBridge's own
/// reason for the second says the same in other words, so it goes too.
pub const INSTALL_STOPPED: &str = "Install stopped.";

/// A path as a person reads it, with the home folder written as `~`. Windows
/// has no such shorthand, so there the path stays whole.
pub fn shown(path: &Path) -> String {
    let home = dirs::home_dir().filter(|_| !cfg!(windows));
    shown_with(path, home.as_deref())
}

/// [`shown`], with `home` as the home folder.
pub fn shown_with(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// "1 error", "2 warnings".
fn counted(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

/// What the checks found before a build: "Check: 0 errors, 1 warning
/// (dismissed)".
pub fn check_line(report: &checks::Report) -> String {
    let warnings = report.warnings.len();
    let dismissed = report.warnings.iter().filter(|w| w.dismissed).count();
    let mut said = format!(
        "Check: {}, {}",
        counted(report.errors.len(), "error"),
        counted(warnings, "warning")
    );
    if dismissed > 0 && dismissed == warnings {
        said.push_str(" (dismissed)");
    } else if dismissed > 0 {
        said.push_str(&format!(" ({dismissed} dismissed)"));
    }
    said
}

/// What the generator drew: "Drew 5 buttons, 1 picture (laser.png)". A
/// picture two buttons share counts once.
pub fn drew_line(layout: &Layout) -> String {
    let mut pictures: Vec<&str> = Vec::new();
    for rel in layout.buttons.iter().filter_map(|b| b.image.as_deref()) {
        let name = Path::new(rel)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(rel);
        if !pictures.contains(&name) {
            pictures.push(name);
        }
    }
    let buttons = counted(layout.buttons.len(), "button");
    if pictures.is_empty() {
        format!("Drew {buttons}")
    } else {
        format!(
            "Drew {buttons}, {} ({})",
            counted(pictures.len(), "picture"),
            pictures.join(", ")
        )
    }
}

/// A size as the window writes it: "48.2 KB", or "1.2 MB" from a million
/// bytes on. The unit is 1000 bytes, as the Finder counts.
pub fn size(bytes: u64) -> String {
    let (amount, unit) = in_units(bytes, bytes);
    format!("{amount} {unit}")
}

/// `bytes` in the unit that suits `whole`, with one decimal.
fn in_units(bytes: u64, whole: u64) -> (String, &'static str) {
    if whole >= 1_000_000 {
        (format!("{:.1}", bytes as f64 / 1_000_000.0), "MB")
    } else {
        (format!("{:.1}", bytes as f64 / 1000.0), "KB")
    }
}

/// A time a step took: "3.8 s".
pub fn seconds(took: Duration) -> String {
    format!("{:.1} s", took.as_secs_f64())
}

/// The upload, as it goes: "Uploading to the band… 62% · 29.9 of 48.2 KB".
/// BandBridge says only the percentage, so what has gone is that share of
/// the file.
pub fn progress_line(percent: u8, total: u64) -> String {
    let sent = total * u64::from(percent.min(100)) / 100;
    let (sent, _) = in_units(sent, total);
    format!(
        "Uploading to the band… {percent}% · {sent} of {}",
        size(total)
    )
}

/// The app's folder name, which names its files: "slides".
fn folder_name(app_dir: &Path) -> String {
    app_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A file of the app as the window names it: "slides/dist/slides.bin".
fn in_app(app_dir: &Path, rest: &str) -> String {
    format!("{}/{rest}", folder_name(app_dir))
}

/// How a build ended, for the window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Built {
    /// Whether the app's .bin was made.
    pub ok: bool,
    /// When the build ended, in milliseconds since 1970: "Built at 14:02".
    pub at: u64,
}

/// Check, save, draw and build the app in `app_dir` with the build tools in
/// `tools`, and say each step to `say` as it happens.
///
/// The layout is saved before the build, and saved again with its version
/// raised only when zmake made the app. A failed build must not change
/// layout.json's version.
pub fn build_app(app_dir: &Path, layout: Layout, tools: &Path, say: &mut dyn FnMut(Said)) -> Built {
    let started = Instant::now();
    let ended = |ok| Built { ok, at: now() };
    let folder = app_dir.parent().map(Path::to_path_buf).unwrap_or_default();
    let used = signals::in_use(&folder);
    let mut current = layout;
    let report = checks::check(&current, app_dir, &used);
    say(Said::now(Tone::Note, check_line(&report)));
    if !report.can_build() {
        for error in &report.errors {
            say(Said::now(Tone::Bad, error.title.clone()));
        }
        return ended(false);
    }

    // Forget dismissals of warnings that no longer appear.
    current
        .dismissed
        .retain(|key| report.warnings.iter().any(|w| &w.key == key));
    let file = app_dir.join("layout.json");
    if let Err(e) = layout::save(&file, &current) {
        say(Said::now(Tone::Bad, e.to_string()));
        return ended(false);
    }
    say(Said::now(
        Tone::Note,
        format!("Saved {}", in_app(app_dir, "layout.json")),
    ));

    let mut bumped = current.clone();
    bumped.version += 1;
    let made = match generate::generate(app_dir, &bumped) {
        Ok(made) => made,
        Err(e) => {
            say(Said::now(Tone::Bad, e.to_string()));
            return ended(false);
        }
    };
    say(Said::now(Tone::Note, drew_line(&bumped)));
    say(Said::now(
        Tone::Step,
        format!("$ zmake {}", shown(&made.project)),
    ));

    let result = zmake::build(tools, app_dir, &made.project, |line| {
        if !line.trim().is_empty() {
            say(Said::now(Tone::Note, format!("zmake: {line}")));
        }
    });
    if !result.ok {
        say(Said::now(Tone::Bad, result.why.unwrap_or_default()));
        return ended(false);
    }
    if let Err(e) = layout::save(&file, &bumped) {
        say(Said::now(Tone::Bad, e.to_string()));
        return ended(false);
    }
    for path in result.bin.iter().chain(&result.zip) {
        let path = Path::new(path);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or_default();
        say(Said::now(
            Tone::Good,
            format!(
                "Built {} · {}",
                in_app(app_dir, &format!("dist/{name}")),
                size(bytes)
            ),
        ));
    }
    if let Some(error) = result.zip_error {
        say(Said::now(Tone::Bad, error));
    }
    say(Said::now(
        Tone::Good,
        format!("Build finished in {}", seconds(started.elapsed())),
    ));
    ended(true)
}

/// How an install ended, for the window. Its card shows for the two stops a
/// person fixes in BandBridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Installed {
    Installed,
    NotRunning,
    NotConnected,
    Stopped,
}

/// Install the app in `app_dir` through BandBridge, and say each step to
/// `say` as it happens.
pub fn install_app(app_dir: &Path, say: &mut dyn FnMut(Said)) -> Installed {
    install_app_with(app_dir, say, |file, each| {
        crate::install::install(file, each)
    })
}

/// [`install_app`], through the BandBridge on `link`: its socket on the Mac,
/// or its pipe on Windows.
pub fn install_app_via(app_dir: &Path, link: &Path, say: &mut dyn FnMut(Said)) -> Installed {
    install_app_with(app_dir, say, |file, each| {
        crate::install::install_via(link, file, each)
    })
}

/// [`install_app`], with `send` to hand the file to BandBridge.
fn install_app_with(
    app_dir: &Path,
    say: &mut dyn FnMut(Said),
    send: impl FnOnce(&Path, &mut dyn FnMut(Heard)) -> Result<(), Stop>,
) -> Installed {
    let name = folder_name(app_dir);
    let bin = app_dir.join("dist").join(format!("{name}.bin"));
    let Ok(file) = std::fs::metadata(&bin) else {
        say(Said::now(Tone::Bad, "Build the app first."));
        return Installed::Stopped;
    };
    let total = file.len();
    let started = Instant::now();
    say(Said::now(
        Tone::Step,
        format!("Install: sending {name}.bin to Mi Band Bridge"),
    ));

    // Two of BandBridge's lines repeat the editor's own: the package's path
    // and size, which the step names and the upload line sizes, and
    // "Installed.", which the end says once BandBridge is done. Every other
    // line stays, in BandBridge's words.
    let sized = format!("{name}.bin: {total} bytes");
    let sent = send(&bin, &mut |heard| match heard {
        Heard::Line(text) if text.ends_with(&sized) || text == "Installed." => {}
        Heard::Line(text) => say(Said::now(Tone::Note, format!("Mi Band Bridge: {text}"))),
        Heard::Progress(percent) => say(Said {
            percent: Some(percent),
            ..Said::now(Tone::Progress, progress_line(percent, total))
        }),
    });
    match sent {
        Ok(()) => {
            say(Said::now(
                Tone::Good,
                format!("Install finished in {}", seconds(started.elapsed())),
            ));
            Installed::Installed
        }
        Err(Stop::NotRunning) => {
            say(Said::now(Tone::Bad, INSTALL_STOPPED));
            Installed::NotRunning
        }
        Err(Stop::NotConnected(_)) => {
            say(Said::now(Tone::Bad, INSTALL_STOPPED));
            Installed::NotConnected
        }
        Err(Stop::Refused(reason)) => {
            say(Said::now(Tone::Bad, format!("Mi Band Bridge: {reason}")));
            Installed::Stopped
        }
        Err(Stop::Lost(words)) => {
            say(Said::now(Tone::Bad, words));
            Installed::Stopped
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::{Finding, Report};
    use crate::layout::Button;

    #[test]
    fn the_home_folder_is_written_as_a_tilde() {
        let folder = Path::new("/Users/ada/Documents/BandApps");
        assert_eq!(
            shown_with(folder, Some(Path::new("/Users/ada"))),
            "~/Documents/BandApps"
        );
        assert_eq!(
            shown_with(Path::new("/Users/ada"), Some(Path::new("/Users/ada"))),
            "~"
        );
        // A folder whose name only starts with the home folder's is not in it.
        assert_eq!(
            shown_with(folder, Some(Path::new("/Users/ad"))),
            "/Users/ada/Documents/BandApps"
        );
        assert_eq!(shown_with(folder, None), "/Users/ada/Documents/BandApps");
    }

    fn finding(dismissed: bool) -> Finding {
        Finding {
            key: "dark-button:3:#3a3d44".to_string(),
            button: Some(3),
            field: None,
            same_signal: Vec::new(),
            title: "“Blank” may be hard to see".to_string(),
            message: "Button 4 is dark gray on the black screen.".to_string(),
            note: None,
            first_note: None,
            action: None,
            dismissed,
        }
    }

    #[test]
    fn the_check_line_counts_and_says_which_warnings_are_dismissed() {
        let report = |errors: usize, warnings: &[bool]| Report {
            errors: (0..errors).map(|_| finding(false)).collect(),
            warnings: warnings.iter().map(|&d| finding(d)).collect(),
            settings: Vec::new(),
        };
        assert_eq!(
            check_line(&report(0, &[true])),
            "Check: 0 errors, 1 warning (dismissed)"
        );
        assert_eq!(
            check_line(&report(0, &[true, true])),
            "Check: 0 errors, 2 warnings (dismissed)"
        );
        assert_eq!(
            check_line(&report(0, &[false, true, false])),
            "Check: 0 errors, 3 warnings (1 dismissed)"
        );
        assert_eq!(check_line(&report(1, &[])), "Check: 1 error, 0 warnings");
        assert_eq!(
            check_line(&report(2, &[false])),
            "Check: 2 errors, 1 warning"
        );
    }

    #[test]
    fn the_drew_line_counts_buttons_and_names_their_pictures() {
        let mut layout = Layout::new("slides");
        layout.buttons.push(Button::new("bb01", "#2f6fde"));
        assert_eq!(drew_line(&layout), "Drew 1 button");
        for signal in ["bb02", "bb03", "bb04", "bb05"] {
            layout.buttons.push(Button::new(signal, "#2f6fde"));
        }
        layout.buttons[2].image = Some("images/laser.png".to_string());
        assert_eq!(drew_line(&layout), "Drew 5 buttons, 1 picture (laser.png)");
        layout.buttons[4].image = Some("images/laser.png".to_string());
        layout.buttons[0].image = Some("images/logo.jpg".to_string());
        assert_eq!(
            drew_line(&layout),
            "Drew 5 buttons, 2 pictures (logo.jpg, laser.png)"
        );
    }

    #[test]
    fn sizes_and_times_read_as_the_window_writes_them() {
        assert_eq!(size(48_213), "48.2 KB");
        assert_eq!(size(512), "0.5 KB");
        assert_eq!(size(1_234_567), "1.2 MB");
        assert_eq!(seconds(Duration::from_millis(3_812)), "3.8 s");
        assert_eq!(seconds(Duration::from_millis(58_249)), "58.2 s");
    }

    #[test]
    fn the_upload_line_gives_its_percentage_and_how_much_has_gone() {
        assert_eq!(
            progress_line(62, 48_213),
            "Uploading to the band… 62% · 29.9 of 48.2 KB"
        );
        assert_eq!(
            progress_line(0, 48_213),
            "Uploading to the band… 0% · 0.0 of 48.2 KB"
        );
        assert_eq!(
            progress_line(100, 48_213),
            "Uploading to the band… 100% · 48.2 of 48.2 KB"
        );
        assert_eq!(
            progress_line(50, 1_234_567),
            "Uploading to the band… 50% · 0.6 of 1.2 MB"
        );
    }

    /// An apps folder with one app, "slides", whose two buttons draw no
    /// warning. The apps folder sits in a parent of its own: a build reads
    /// the signals one level above the apps folder too, and the system's
    /// temp folder, shared with other tests, must not be that level.
    #[cfg(unix)]
    fn slides(buttons: &[(&str, &str)]) -> (tempfile::TempDir, std::path::PathBuf, Layout) {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("apps").join("slides");
        std::fs::create_dir_all(app.join("images")).unwrap();
        let mut layout = Layout::new("slides");
        layout.columns = 1;
        for (label, signal) in buttons {
            let mut button = Button::new(signal, "#2f6fde");
            button.label = label.to_string();
            layout.buttons.push(button);
        }
        layout::save(&app.join("layout.json"), &layout).unwrap();
        (root, app, layout)
    }

    /// Tools whose Python stands in for zmake, as `zmake`'s tests make them.
    #[cfg(unix)]
    fn tools(script: &str) -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt;
        let tools = tempfile::tempdir().unwrap();
        let python = zmake::python_in(tools.path());
        std::fs::create_dir_all(python.parent().unwrap()).unwrap();
        std::fs::write(&python, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).unwrap();
        tools
    }

    /// A zmake that makes a .bin of 48,213 bytes and a .zip of 51,000. A shell
    /// script, so only the tests that run one use it.
    #[cfg(unix)]
    const A_GOOD_ZMAKE: &str = r#"echo "Processing app.json:"
echo "  Done" >&2
mkdir -p "$3/dist"
head -c 48213 /dev/zero > "$3/dist/slides.bin"
head -c 51000 /dev/zero > "$3/dist/slides.zip"
echo "Completed without error.""#;

    fn texts(said: &[Said]) -> Vec<(Tone, String)> {
        said.iter().map(|s| (s.tone, s.text.clone())).collect()
    }

    #[cfg(unix)]
    #[test]
    fn a_build_says_each_step_as_it_happens() {
        let (_root, app, layout) = slides(&[("Up", "bb01"), ("Down", "bb02")]);
        let tools = tools(A_GOOD_ZMAKE);
        let mut said = Vec::new();
        let built = build_app(&app, layout.clone(), tools.path(), &mut |s| said.push(s));

        assert!(built.ok, "{said:?}");
        let mut lines = texts(&said);
        let last = lines.pop().unwrap();
        assert_eq!(last.0, Tone::Good);
        assert!(
            last.1.starts_with("Build finished in ") && last.1.ends_with(" s"),
            "{}",
            last.1
        );
        let project = shown(&app.join("generated/slides"));
        assert_eq!(
            lines,
            [
                (Tone::Note, "Check: 0 errors, 0 warnings".to_string()),
                (Tone::Note, "Saved slides/layout.json".to_string()),
                (Tone::Note, "Drew 2 buttons".to_string()),
                (Tone::Step, format!("$ zmake {project}")),
                (Tone::Note, "zmake: Processing app.json:".to_string()),
                (Tone::Note, "zmake:   Done".to_string()),
                (Tone::Note, "zmake: Completed without error.".to_string()),
                (
                    Tone::Good,
                    "Built slides/dist/slides.bin · 48.2 KB".to_string()
                ),
                (
                    Tone::Good,
                    "Built slides/dist/slides.zip · 51.0 KB".to_string()
                ),
            ]
        );
        // Each line carries its moment, in order, and none is progress.
        assert!(said.windows(2).all(|w| w[0].at <= w[1].at));
        assert!(said.iter().all(|s| s.percent.is_none()));
        // The version goes up with a build that made the app.
        let saved = layout::load(&app.join("layout.json")).unwrap();
        assert_eq!(saved.version, layout.version + 1);
        assert!(app.join("dist/slides.bin").is_file());
    }

    /// Review Focus 4: zmake's own words show, in the order zmake wrote
    /// them, with FAILED last, the build ends, and the version stays where
    /// it was. This zmake says "EOFError" if it gets no key after FAILED.
    #[cfg(unix)]
    #[test]
    fn a_build_that_fails_in_zmake_shows_zmakes_words_and_keeps_the_version() {
        let (_root, app, layout) = slides(&[("Up", "bb01")]);
        let tools = tools(
            r#"echo "Processing assets:"
echo "Traceback (most recent call last):" >&2
echo "IsADirectoryError: [Errno 21] Is a directory" >&2
echo "FAILED"
read key || { echo "EOFError: EOF when reading a line" >&2; exit 1; }"#,
        );
        let mut said = Vec::new();
        let built = build_app(&app, layout.clone(), tools.path(), &mut |s| said.push(s));

        assert!(!built.ok);
        let lines = texts(&said);
        let zmake: Vec<&str> = lines
            .iter()
            .filter(|(_, text)| text.starts_with("zmake: "))
            .map(|(_, text)| text.as_str())
            .collect();
        assert_eq!(
            zmake,
            [
                "zmake: Processing assets:",
                "zmake: Traceback (most recent call last):",
                "zmake: IsADirectoryError: [Errno 21] Is a directory",
                "zmake: FAILED",
            ]
        );
        assert_eq!(
            lines.last().unwrap(),
            &(Tone::Bad, "zmake did not make slides.bin.".to_string())
        );
        assert_eq!(
            layout::load(&app.join("layout.json")).unwrap().version,
            layout.version
        );
        assert!(!app.join("dist").exists());
    }

    /// An error the checks found stops the build before zmake runs, and says
    /// what it is.
    #[cfg(unix)]
    #[test]
    fn a_build_with_an_error_stops_before_zmake() {
        let (_root, app, layout) = slides(&[("Up", "bb04"), ("Down", "bb04")]);
        let ran = app.join("zmake ran");
        let tools = tools(&format!("touch '{}'", ran.display()));
        let mut said = Vec::new();
        let built = build_app(&app, layout, tools.path(), &mut |s| said.push(s));

        assert!(!built.ok);
        assert_eq!(
            texts(&said),
            [
                (Tone::Note, "Check: 1 error, 0 warnings".to_string()),
                (Tone::Bad, "Two buttons send bb04".to_string()),
            ]
        );
        assert!(!ran.exists());
    }

    /// An install of an app never built asks for a build, and reaches no
    /// BandBridge.
    #[test]
    fn an_install_before_a_build_asks_for_one() {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("slides");
        std::fs::create_dir_all(&app).unwrap();
        let mut said = Vec::new();
        let ended = install_app_with(&app, &mut |s| said.push(s), |_, _| {
            panic!("it reached BandBridge")
        });
        assert_eq!(ended, Installed::Stopped);
        assert_eq!(
            texts(&said),
            [(Tone::Bad, "Build the app first.".to_string())]
        );
    }
}
