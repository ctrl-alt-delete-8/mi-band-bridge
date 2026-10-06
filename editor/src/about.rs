//! What the About dialog shows and opens: the editor's name and version, and
//! the project's two licenses and its notices, in its public repository.

/// The project's public repository, which holds the licenses and the notices.
const REPOSITORY: &str = "https://github.com/ctrl-alt-delete-8/mi-band-bridge";

/// This build's version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The address of each page the About dialog opens, by its name. The page
/// asks by name, so it can open these pages and no others.
pub fn page(name: &str) -> Option<String> {
    let file = match name {
        "app-license" => "LICENSE-APP.md",
        "source-license" => "LICENSE",
        "notices" => "THIRD-PARTY-NOTICES",
        _ => return None,
    };
    Some(format!("{REPOSITORY}/blob/main/{file}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The About dialog opens the project's two licenses and its notices,
    /// in the public repository, and nothing else.
    #[test]
    fn the_about_links_open_the_licenses_and_the_notices() {
        let repo = "https://github.com/ctrl-alt-delete-8/mi-band-bridge/blob/main";
        assert_eq!(page("app-license"), Some(format!("{repo}/LICENSE-APP.md")));
        assert_eq!(page("source-license"), Some(format!("{repo}/LICENSE")));
        assert_eq!(page("notices"), Some(format!("{repo}/THIRD-PARTY-NOTICES")));
        assert_eq!(page("https://example.com"), None);
    }

    /// People see the name Mi Band App Editor and the version 1.0.0: in the
    /// window and in the Windows installer. Its identifier stays.
    #[test]
    fn the_app_is_mi_band_app_editor_1_0_0() {
        assert_eq!(VERSION, "1.0.0");
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["productName"], "Mi Band App Editor");
        assert_eq!(conf["version"], "1.0.0");
        assert_eq!(conf["app"]["windows"][0]["title"], "Mi Band App Editor");
        assert_eq!(conf["identifier"], "dev.bandbridge.editor");
        let windows: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.windows.conf.json")).unwrap();
        assert_eq!(windows["app"]["windows"][0]["title"], "Mi Band App Editor");
        let page = include_str!("../ui/index.html");
        assert!(page.contains("<title>Mi Band App Editor</title>"));
        assert!(page.contains("<h1>Mi Band App Editor</h1>"));
    }

    /// The Mac release builds the editor as an app bundle, with the hardened
    /// runtime and no entitlements. It runs the build tools' Python and opens
    /// pages in the browser, and neither needs one. Windows makes installers.
    #[test]
    fn the_mac_release_bundles_the_editor_as_an_app() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["bundle"]["active"], true);
        assert_eq!(conf["bundle"]["targets"], serde_json::json!(["app"]));
        assert_eq!(conf["bundle"]["macOS"]["hardenedRuntime"], true);
        assert!(conf["bundle"]["macOS"]["entitlements"].is_null());
        let windows: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.windows.conf.json")).unwrap();
        assert_eq!(windows["bundle"]["targets"], serde_json::json!(["nsis"]));
    }
}
