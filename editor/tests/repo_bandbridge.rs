//! BandBridge's own words and rules, read from its source, where the editor
//! depends on them.
//!
//! The editor does not use the `bandbridge` crate, so no code ties the two.
//! These tests read BandBridge's source instead, so a reworded message or a
//! renamed pipe fails here rather than silently in the window.

use std::path::Path;

/// The editor shows its card "The band is not connected." when BandBridge
/// fails an install because it holds no link to the band: `refused` in
/// `src/install.rs` looks for "not connected" in BandBridge's reason. That
/// reason comes from `install_here` in BandBridge's `src/supervisor.rs`.
#[test]
fn bandbridge_says_the_band_is_not_connected_when_it_cannot_install() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the editor sits in the repo");
    let source = std::fs::read_to_string(repo.join("src/supervisor.rs")).unwrap();
    let start = source
        .find("async fn install_here(")
        .expect("BandBridge installs in install_here");
    let length = source[start..]
        .find("async fn installed(")
        .expect("install_here is followed by installed");
    let install_here = &source[start..start + length];
    assert!(
        install_here.contains("the band is not connected"),
        "install_here no longer says \"the band is not connected\", so the editor's \
         card for it will not show. Change `refused` in editor/src/install.rs with it."
    );
}

/// The editor finds BandBridge's pipe on Windows by BandBridge's own rule
/// (W6): `\\.\pipe\bandbridge-` and the SID of the user the process runs as,
/// read from the process token. Both sides write the rule, so both must say
/// the same, or the editor would look for a pipe nobody makes.
#[test]
fn the_editor_names_bandbridges_pipe_by_the_same_rule() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the editor sits in the repo");
    let bandbridge = std::fs::read_to_string(repo.join("src/platform/link/windows.rs")).unwrap();
    let editor = std::fs::read_to_string(repo.join("editor/src/install.rs")).unwrap();
    for (side, source) in [("BandBridge", &bandbridge), ("the editor", &editor)] {
        for part in [
            r#"r"\\.\pipe\bandbridge-{}""#,
            "OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY",
            "TokenUser",
            "ConvertSidToStringSidW(user.User.Sid",
        ] {
            assert!(
                source.contains(part),
                "{side} no longer names the pipe with {part}. Change both sides together: \
                 src/platform/link/windows.rs and editor/src/install.rs."
            );
        }
    }
}
