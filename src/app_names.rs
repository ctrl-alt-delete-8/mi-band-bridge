//! Names for the apps on the band.
//!
//! The band reports an installed app as an id in hex and nothing else, so a
//! list shows `10BD37D4` where a name belongs. This file is the one record of
//! names: each install through BandBridge reads the id and the name from the
//! manifest beside the package, and remembers them here. An app with no entry
//! keeps its id.
//!
//! Nothing else names an app, not even this project's own: one record means
//! one place to look, and one place to fix.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where the learned names live, in BandBridge's folder beside the config.
pub fn path() -> Option<PathBuf> {
    Some(crate::config::folder()?.join("app-names.toml"))
}

/// Every name the record holds, by app id in upper case.
///
/// Read this once for a listing. It touches the disk.
pub fn all() -> BTreeMap<String, String> {
    learned()
        .into_iter()
        .map(|(id, name)| (id.to_ascii_uppercase(), name))
        .collect()
}

/// The name for one id in a map from `all`.
pub fn in_map(names: &BTreeMap<String, String>, id: &str) -> Option<String> {
    names.get(&id.to_ascii_uppercase()).cloned()
}

fn learned() -> BTreeMap<String, String> {
    let Some(path) = path() else {
        return BTreeMap::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    toml::from_str(&text).unwrap_or_default()
}

/// Remember that this id is called this.
pub fn remember(id: &str, name: &str) -> std::io::Result<()> {
    let Some(path) = path() else {
        return Ok(());
    };
    let mut names = learned();
    names.insert(id.to_ascii_uppercase(), name.to_string());
    let text = toml::to_string(&names)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)
}

/// The id and name of the app a package belongs to, read from the manifest
/// that sits beside it.
///
/// Two layouts, because there are two kinds of app folder:
///
/// ```text
/// band-app/dist/band-app.bin        -> band-app/app.json
/// apps/slides/dist/slides.bin       -> apps/slides/generated/slides/app.json
/// ```
///
/// A package with no manifest nearby gives None, and the app keeps its id.
pub fn from_package(package: &Path) -> Option<(String, String)> {
    // dist/<name>.bin, so the app folder is two levels up.
    let app_dir = package.parent()?.parent()?;
    let direct = app_dir.join("app.json");
    if let Some(found) = from_manifest(&direct) {
        return Some(found);
    }
    // The editor writes a folder per app under generated/.
    let generated = std::fs::read_dir(app_dir.join("generated")).ok()?;
    generated
        .flatten()
        .find_map(|entry| from_manifest(&entry.path().join("app.json")))
}

/// The id and name in one Zepp OS manifest.
pub fn from_manifest(manifest: &Path) -> Option<(String, String)> {
    let text = std::fs::read_to_string(manifest).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let app = value.get("app")?;
    let id = app.get("appId")?.as_u64()?;
    let name = app.get("appName")?.as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    Some((format!("{id:08X}"), name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(dir: &Path, id: u64, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("app.json"),
            format!("{{\"app\": {{\"appId\": {id}, \"appName\": \"{name}\"}}}}"),
        )
        .unwrap();
    }

    /// A hand written app keeps its manifest in the app folder.
    #[test]
    fn a_package_finds_the_manifest_beside_it() {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("band-app");
        manifest(&app, 1_491_417_013, "BandBridge");
        std::fs::create_dir_all(app.join("dist")).unwrap();
        let package = app.join("dist/band-app.bin");
        std::fs::write(&package, b"not a real package").unwrap();

        assert_eq!(
            from_package(&package),
            Some(("58E537B5".to_string(), "BandBridge".to_string()))
        );
    }

    /// An app from the editor keeps its manifest under generated/<name>/.
    #[test]
    fn a_package_finds_a_generated_manifest() {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("testbandapp");
        manifest(&app.join("generated/testbandapp"), 280_836_052, "testbandapp");
        std::fs::create_dir_all(app.join("dist")).unwrap();
        let package = app.join("dist/testbandapp.bin");
        std::fs::write(&package, b"not a real package").unwrap();

        assert_eq!(
            from_package(&package),
            Some(("10BD37D4".to_string(), "testbandapp".to_string()))
        );
    }

    #[test]
    fn a_package_on_its_own_gives_no_name() {
        let root = tempfile::tempdir().unwrap();
        let package = root.path().join("loose.bin");
        std::fs::write(&package, b"not a real package").unwrap();
        assert_eq!(from_package(&package), None);
    }

    #[test]
    fn the_names_live_beside_the_config() {
        let names = path().expect("a home folder");
        let config = crate::config::Config::path().expect("a home folder");
        assert_eq!(names.parent(), config.parent());
        assert!(names.ends_with("BandBridge/app-names.toml"), "{}", names.display());
    }

    #[test]
    fn a_manifest_without_a_name_gives_none() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("app.json"), "{\"app\": {\"appId\": 1}}").unwrap();
        assert_eq!(from_manifest(&root.path().join("app.json")), None);
    }
}
