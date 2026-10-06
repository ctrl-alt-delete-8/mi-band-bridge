//! Which signals apps already send.

use crate::layout;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Use {
    pub signal: String,
    pub app: String,
    /// The app's folder name.
    pub dir: Option<String>,
}

/// The file a hand-written app uses to say what it sends.
///
/// An app the editor did not generate has no `layout.json`, and its signals can
/// sit behind a variable: `band-app-arrows` sends `[0xbb, second]`, where
/// `second` is an argument. Reading the code is guesswork, so the app declares
/// the answer. The rules in `skills/` ask every hand-written app for one.
pub const DECLARATION: &str = "signals.json";

#[derive(Deserialize)]
struct Declared {
    signals: Vec<String>,
}

/// Every signal in use.
///
/// Apps sit one level under `folder`. The editor looks one level above it as
/// well, because this repository keeps its hand-written apps beside the apps
/// folder. A folder counts only when it holds a `layout.json` or a
/// `signals.json`, so nothing else in either place is read.
pub fn in_use(folder: &Path) -> Vec<Use> {
    let mut uses = Vec::new();
    let mut roots = vec![folder.to_path_buf()];
    roots.extend(folder.parent().map(Path::to_path_buf));
    for root in roots {
        collect(&root, &mut uses);
    }
    uses
}

fn collect(root: &Path, uses: &mut Vec<Use>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    dirs.sort();
    for dir in dirs {
        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned());
        if dir.join("layout.json").is_file() {
            let Ok(app) = layout::load(&dir.join("layout.json")) else {
                continue;
            };
            for button in &app.buttons {
                if let Some(signal) = layout::normalize_signal(&button.signal) {
                    uses.push(Use { signal, app: app.name.clone(), dir: name.clone() });
                }
            }
        } else if let Some(declared) = declaration(&dir) {
            let app = manifest_name(&dir)
                .or_else(|| name.clone())
                .unwrap_or_else(|| "an app".to_string());
            for signal in &declared.signals {
                if let Some(signal) = layout::normalize_signal(signal) {
                    uses.push(Use { signal, app: app.clone(), dir: name.clone() });
                }
            }
        }
    }
}

fn declaration(dir: &Path) -> Option<Declared> {
    let text = std::fs::read_to_string(dir.join(DECLARATION)).ok()?;
    serde_json::from_str(&text).ok()
}

/// The name the Zepp OS manifest gives the app, so the name is not written
/// twice and cannot go stale in one of the two places.
fn manifest_name(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("app.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(value.get("app")?.get("appName")?.as_str()?.to_string())
}

/// The first `bbNN` from `bb04` that nothing uses.
pub fn next_free(used: &[Use]) -> Option<String> {
    (0x04..=0xffu32)
        .map(|n| format!("bb{n:02x}"))
        .find(|candidate| !used.iter().any(|u| &u.signal == candidate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Button, Layout, save};

    /// An apps folder inside a parent of its own. The scan reads the folder
    /// above too, and a folder made straight in the system's temp folder
    /// would have that shared folder above it, where other tests leave apps
    /// while the suite runs.
    fn apps_folder() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("apps");
        std::fs::create_dir_all(&folder).unwrap();
        (root, folder)
    }

    fn app(folder: &Path, dir: &str, name: &str, signals: &[&str]) {
        let path = folder.join(dir);
        std::fs::create_dir_all(&path).unwrap();
        let mut layout = Layout::new(name);
        for s in signals {
            layout.buttons.push(Button::new(s, "#000000"));
        }
        save(&path.join("layout.json"), &layout).unwrap();
    }

    /// A hand-written app has no layout.json. It declares what it sends, and
    /// the name comes from its Zepp OS manifest so it is written once.
    #[test]
    fn a_hand_written_app_declares_what_it_sends() {
        let (_root, folder) = apps_folder();
        let dir = folder.join("band-app-arrows");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(DECLARATION), r#"{"signals": ["BB02", "bb03"]}"#).unwrap();
        std::fs::write(dir.join("app.json"), r#"{"app": {"appName": "BandArrows"}}"#).unwrap();

        let used = in_use(&folder);
        let found = used.iter().find(|u| u.signal == "bb02").expect("bb02");
        assert_eq!(found.app, "BandArrows");
        assert_eq!(found.dir.as_deref(), Some("band-app-arrows"));
        assert!(used.iter().any(|u| u.signal == "bb03"));
    }

    /// The hand-written apps of this repository sit beside the apps folder,
    /// not inside it, so the folder above the one in hand counts too.
    #[test]
    fn the_folder_above_counts_as_well() {
        let root = tempfile::tempdir().unwrap();
        let apps = root.path().join("apps");
        std::fs::create_dir_all(&apps).unwrap();
        let beside = root.path().join("band-app");
        std::fs::create_dir_all(&beside).unwrap();
        std::fs::write(beside.join(DECLARATION), r#"{"signals": ["bb01"]}"#).unwrap();

        let used = in_use(&apps);
        assert!(used.iter().any(|u| u.signal == "bb01" && u.app == "band-app"), "{used:?}");
    }

    /// Nothing else in either folder is read.
    #[test]
    fn a_folder_that_declares_nothing_is_not_an_app() {
        let (_root, folder) = apps_folder();
        std::fs::create_dir_all(folder.join("notes")).unwrap();
        std::fs::write(folder.join("notes/app.json"), "{}").unwrap();
        assert!(in_use(&folder).is_empty());
    }

    #[test]
    fn every_layout_in_the_folder_counts() {
        let (_root, folder) = apps_folder();
        app(&folder, "slides", "Slides", &["BB04", "bb05"]);
        let used = in_use(&folder);
        let found = used.iter().find(|u| u.signal == "bb04").unwrap();
        assert_eq!(found.app, "Slides");
        assert_eq!(found.dir.as_deref(), Some("slides"));
    }

    #[test]
    fn the_next_free_signal_skips_used_ones() {
        let (_root, folder) = apps_folder();
        assert_eq!(next_free(&in_use(&folder)).as_deref(), Some("bb04"));
        app(&folder, "slides", "Slides", &["bb04", "bb05"]);
        assert_eq!(next_free(&in_use(&folder)).as_deref(), Some("bb06"));
    }

    #[test]
    fn an_unreadable_layout_is_skipped() {
        let (_root, folder) = apps_folder();
        std::fs::create_dir_all(folder.join("broken")).unwrap();
        std::fs::write(folder.join("broken/layout.json"), "{").unwrap();
        assert!(in_use(&folder).is_empty());
    }
}
