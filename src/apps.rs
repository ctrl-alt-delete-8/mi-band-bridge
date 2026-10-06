//! Listing and removing the apps on the band.
//!
//! The wire format lives in `protocol::apps`. This is the part that talks to
//! the band and decides what is safe to do.
//!
//! One rule runs through it: **the band names an app by id and nothing else**,
//! so a removal is only allowed for an id the band itself has just listed.
//! A typed id that is one digit out would otherwise delete a watch face.

use crate::error::BandError;
use crate::app_names;
use crate::protocol::apps::{self, InstalledApp};
use crate::protocol::watchface;
use crate::session::Session;
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::Duration;

/// How long to wait for the band to answer.
const REPLY_WAIT: Duration = Duration::from_secs(8);

/// One installed app, ready to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// The id as the band gives it, in upper case hex.
    pub id: String,
    /// The same id as a number, which is what a manifest holds.
    pub number: Option<u32>,
    pub version: String,
    /// The name, when this project built the app or installed it. The band
    /// reports none.
    pub name: Option<String>,
    /// A watch face, rather than a program. The band gives both the same kind
    /// of id, and only the watch face list tells them apart.
    pub is_watchface: bool,
    /// The face the band is wearing. Removing it would leave the screen with
    /// no face, so it is refused.
    pub in_use: bool,
}

impl Entry {
    /// What to call the app in a few words: its name, or its id when no name
    /// is known.
    pub fn name_or_id(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    /// A line for a person: the name when we know it, the id when we do not.
    pub fn label(&self) -> String {
        let head = match &self.name {
            Some(name) => format!("{name} ({}) version {}", self.id, self.version),
            None => format!("{} version {}", self.id, self.version),
        };
        match (self.is_watchface, self.in_use) {
            (true, true) => format!("{head} [watch face, in use]"),
            (true, false) => format!("{head} [watch face]"),
            _ => head,
        }
    }
}

fn describe(app: &InstalledApp, names: &BTreeMap<String, String>, faces: &Faces) -> Entry {
    // The band pads an id to eight digits in the watch face list and trims it
    // in the app list, so compare the numbers rather than the text.
    let number = app.number();
    Entry {
        name: app_names::in_map(names, &app.id),
        is_watchface: number.is_some_and(|n| faces.installed.contains(&n)),
        in_use: number.is_some_and(|n| faces.current == Some(n)),
        number,
        id: app.id.clone(),
        version: app.version.clone(),
    }
}

/// What the band says about its watch faces. Empty when it does not answer,
/// which costs a label rather than the whole listing.
#[derive(Debug, Default)]
struct Faces {
    installed: Vec<u32>,
    current: Option<u32>,
}

async fn faces(session: &mut Session) -> Faces {
    let mut found = Faces::default();
    for (request, want_list) in [
        (watchface::list_request(), true),
        (watchface::current_request(), false),
    ] {
        if session.send(watchface::ENDPOINT, &request, true).await.is_err() {
            continue;
        }
        let deadline = std::time::Instant::now() + REPLY_WAIT;
        while std::time::Instant::now() < deadline {
            let Ok(message) = session.next_message(Duration::from_secs(2)).await else {
                continue;
            };
            if message.endpoint != watchface::ENDPOINT {
                continue;
            }
            if want_list {
                if let Some(ids) = watchface::parse_list(&message.payload) {
                    found.installed = ids.iter().filter_map(|id| u32::from_str_radix(id, 16).ok()).collect();
                    break;
                }
            } else if let Some(id) = watchface::parse_current(&message.payload) {
                found.current = u32::from_str_radix(&id, 16).ok();
                break;
            }
        }
    }
    found
}

/// The apps installed on the band.
///
/// By default this is the apps we can name, which is the apps installed from
/// here. The band holds other packages: watch faces, and whatever came with
/// it. They have no name, nothing here put them there, and removing them is
/// either refused or a bad idea, so showing them only invites a mistake.
///
/// `everything` shows them anyway, for the times a person is asking what the
/// band actually holds.
pub async fn list(session: &mut Session, everything: bool) -> Result<Vec<Entry>, BandError> {
    let names = app_names::all();
    let installed = fetch(session).await?;
    let faces = faces(session).await;
    let all: Vec<Entry> = installed
        .iter()
        .map(|app| describe(app, &names, &faces))
        .collect();
    if everything {
        return Ok(all);
    }
    Ok(all.into_iter().filter(|e| e.name.is_some()).collect())
}

/// How many packages the band holds, named or not.
pub async fn count(session: &mut Session) -> Result<usize, BandError> {
    Ok(fetch(session).await?.len())
}

/// The lines a person reads: the apps, then a word about what is not shown.
pub async fn list_lines(session: &mut Session, everything: bool) -> Result<Vec<String>, BandError> {
    let shown = list(session, everything).await?;
    let mut lines: Vec<String> = shown.iter().map(Entry::label).collect();
    if !everything {
        let held = fetch(session).await?.len();
        let hidden = held.saturating_sub(shown.len());
        if hidden > 0 {
            lines.push(format!(
                "{hidden} more package(s) on the band are not shown: the watch face, and whatever came with it. Nothing here installed them."
            ));
        }
    }
    Ok(lines)
}

async fn fetch(session: &mut Session) -> Result<Vec<InstalledApp>, BandError> {
    session
        .send(apps::ENDPOINT, &apps::list_request(), false)
        .await?;

    let deadline = std::time::Instant::now() + REPLY_WAIT;
    while std::time::Instant::now() < deadline {
        let Ok(message) = session.next_message(Duration::from_secs(2)).await else {
            continue;
        };
        if message.endpoint != apps::ENDPOINT {
            continue;
        }
        if let Some(found) = apps::parse_list(&message.payload) {
            return Ok(found);
        }
    }
    Err(BandError::Apps(
        "the band did not answer with its app list".to_string(),
    ))
}

/// Remove one app, named by the id the band reports, and give back what was
/// removed.
///
/// The list is read again first. An id the band does not list is refused,
/// because the band takes an id and gives back no name: a wrong one would
/// delete something else without a word.
pub async fn remove(session: &mut Session, id: &str) -> Result<Entry, BandError> {
    let wanted = id.trim().trim_start_matches("0x").to_ascii_uppercase();
    let installed = fetch(session).await?;
    let Some(app) = installed
        .iter()
        .find(|a| a.id.eq_ignore_ascii_case(&wanted))
    else {
        let known: Vec<String> = installed.iter().map(|a| a.id.clone()).collect();
        return Err(BandError::Apps(format!(
            "the band has no app {wanted}. It lists: {}",
            known.join(", ")
        )));
    };
    let Some(number) = app.number() else {
        return Err(BandError::Apps(format!("{} is not a hex id", app.id)));
    };
    let entry = describe(app, &app_names::all(), &faces(session).await);
    // Removing the face the band is wearing leaves the screen with none, and
    // the band does not offer a way back from here.
    if entry.in_use {
        return Err(BandError::Apps(format!(
            "{} is the watch face the band is wearing. Change the face on the band first.",
            entry.label()
        )));
    }
    let shown = entry.label();

    session
        .send(apps::ENDPOINT, &apps::delete_request(number), false)
        .await?;

    // The band acknowledges, and the acknowledgement carries no status, so the
    // list is read again rather than trusted.
    let deadline = std::time::Instant::now() + REPLY_WAIT;
    while std::time::Instant::now() < deadline {
        let Ok(message) = session.next_message(Duration::from_secs(2)).await else {
            continue;
        };
        if message.endpoint == apps::ENDPOINT && apps::is_delete_reply(&message.payload) {
            break;
        }
    }

    let after = fetch(session).await?;
    if after.iter().any(|a| a.id.eq_ignore_ascii_case(&wanted)) {
        return Err(BandError::Apps(format!(
            "the band still lists {shown}. It refused the removal."
        )));
    }
    Ok(entry)
}

/// The line a person reads after a removal.
pub fn removed_line(entry: &Entry) -> String {
    format!("removed {}", entry.label())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names come from a map the caller reads once, so this test needs no
    /// files and cannot be changed by what is installed on the machine.
    #[test]
    fn a_known_app_shows_its_name_and_an_unknown_one_its_id() {
        let names = BTreeMap::from([("10BD37D4".to_string(), "testbandapp".to_string())]);

        let none = Faces::default();
        let known = describe(&InstalledApp { id: "10BD37D4".into(), version: "1.0.2".into() }, &names, &none);
        assert_eq!(known.number, Some(280_836_052));
        assert_eq!(known.label(), "testbandapp (10BD37D4) version 1.0.2");

        let stranger = describe(&InstalledApp { id: "FF31".into(), version: "1.3.0".into() }, &names, &none);
        assert_eq!(stranger.name, None);
        assert_eq!(stranger.label(), "FF31 version 1.3.0");
    }

    /// The event log calls a removed app by its name, or by its id when it
    /// has none. The line for a person keeps the id and the version.
    #[test]
    fn a_removed_app_goes_by_its_name_or_its_id() {
        let names = BTreeMap::from([("58E538D5".to_string(), "BandArrows".to_string())]);
        let none = Faces::default();
        let named = describe(
            &InstalledApp {
                id: "58E538D5".into(),
                version: "1.0.4".into(),
            },
            &names,
            &none,
        );
        assert_eq!(named.name_or_id(), "BandArrows");
        assert_eq!(
            removed_line(&named),
            "removed BandArrows (58E538D5) version 1.0.4"
        );

        let stranger = describe(
            &InstalledApp {
                id: "2D38".into(),
                version: "1.0".into(),
            },
            &names,
            &none,
        );
        assert_eq!(stranger.name_or_id(), "2D38");
        assert_eq!(removed_line(&stranger), "removed 2D38 version 1.0");
    }

    /// The band lists a watch face and an app the same way. Only the watch
    /// face list tells them apart, and the face in use says so loudly.
    #[test]
    fn a_watch_face_is_labeled_and_the_one_in_use_more_so() {
        let names = BTreeMap::new();
        let faces = Faces { installed: vec![0x0000FF31], current: Some(0x0000FF31) };
        let face = describe(&InstalledApp { id: "FF31".into(), version: "1.3.0".into() }, &names, &faces);
        assert!(face.is_watchface);
        assert!(face.in_use);
        assert_eq!(face.label(), "FF31 version 1.3.0 [watch face, in use]");

        let spare = Faces { installed: vec![0x0000FF31, 0x00002D38], current: Some(0x0000FF31) };
        let other = describe(&InstalledApp { id: "2D38".into(), version: "1.0".into() }, &names, &spare);
        assert_eq!(other.label(), "2D38 version 1.0 [watch face]");

        // An app is left alone.
        let app = describe(&InstalledApp { id: "58E537B5".into(), version: "1.0.12".into() }, &names, &faces);
        assert!(!app.is_watchface);
        assert_eq!(app.label(), "58E537B5 version 1.0.12");
    }
}
