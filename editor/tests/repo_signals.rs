//! This repository's own apps, read the way the editor reads them.
//!
//! The unit tests use folders they build themselves. This one uses the real
//! `band-app` and `band-app-arrows`, so a declaration that goes missing or
//! stops matching its manifest fails the build.

use band_editor::signals::in_use;
use std::path::Path;

#[test]
fn the_hand_written_apps_declare_the_signals_they_send() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("the editor sits in the repo");
    let used = in_use(&repo.join("apps"));
    for (signal, app) in [("bb01", "BandBridge"), ("bb02", "BandArrows"), ("bb03", "BandArrows")] {
        assert!(
            used.iter().any(|u| u.signal == signal && u.app == app),
            "{signal} of {app} is missing from {used:?}"
        );
    }
}
