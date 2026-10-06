//! Reading and arranging the band's screen lists.
//!
//! These functions return the lines to show rather than printing them, so the
//! command line, the socket, and the window all get the same words from the
//! same code.

use crate::error::BandError;
use serde::Serialize;
use crate::menu_names;
use crate::session::Session;
use crate::shortcuts::{self, DisplayItem, ListKind};
use std::collections::BTreeMap;
use std::time::Duration;

/// How long to wait for the band to answer a list request.
const REPLY_WAIT: Duration = Duration::from_secs(6);

/// Which list a name belongs to.
pub fn kind_from_name(list: &str) -> Result<ListKind, BandError> {
    match list {
        "menu" => Ok(ListKind::Menu),
        "shortcuts" => Ok(ListKind::Shortcuts),
        "control" | "control-center" => Ok(ListKind::ControlCenter),
        other => Err(BandError::DisplayList(format!(
            "unknown list {other}. Use menu, shortcuts, or control."
        ))),
    }
}

/// The name table for a list, so a person can type "music" rather than hex.
fn table_for(kind: ListKind) -> &'static [(&'static str, &'static str)] {
    match kind {
        ListKind::Menu => menu_names::MENU,
        ListKind::Shortcuts => menu_names::SHORTCUTS,
        ListKind::ControlCenter => menu_names::CONTROL_CENTER,
    }
}

/// Turn what a person typed into the id the band uses. A name is looked up in
/// that list's table; anything else is taken as an id already.
fn resolve(kind: ListKind, wanted: &str) -> String {
    menu_names::id_for(table_for(kind), wanted)
        .map(str::to_string)
        .unwrap_or_else(|| wanted.to_uppercase())
}

/// How to show one entry: its name when the band has a known screen there.
/// Otherwise it is an installed app, shown by the name `names` holds for it,
/// or by its id.
fn describe(kind: ListKind, item: &DisplayItem, names: &BTreeMap<String, String>) -> String {
    if let Some(name) = menu_names::name_for(table_for(kind), &item.id) {
        return format!("{name} ({})", item.id);
    }
    match crate::app_names::in_map(names, &item.id) {
        Some(name) => format!("{name} (app)"),
        None => format!("{} (app)", item.id),
    }
}

/// The entries a list shows, in order.
fn shown_lines(kind: ListKind, items: &[DisplayItem]) -> Vec<String> {
    let mut shown: Vec<&DisplayItem> = items.iter().filter(|i| i.shown()).collect();
    shown.sort_by_key(|i| i.position);

    let mut lines = vec![format!("{} now shows, in order:", kind.label())];
    if shown.is_empty() {
        lines.push("  nothing".to_string());
    }
    let names = crate::app_names::all();
    for item in shown {
        lines.push(format!("  {}", describe(kind, item, &names)));
    }
    lines
}

/// Ask the band for one list.
async fn fetch(session: &mut Session, kind: ListKind) -> Result<Vec<DisplayItem>, BandError> {
    session
        .send(shortcuts::ENDPOINT, &shortcuts::request(kind), true)
        .await?;

    let deadline = std::time::Instant::now() + REPLY_WAIT;
    while std::time::Instant::now() < deadline {
        let Ok(message) = session.next_message(Duration::from_secs(2)).await else {
            continue;
        };
        if message.endpoint != shortcuts::ENDPOINT {
            continue;
        }
        match shortcuts::parse_response(&message.payload) {
            Ok((got, items)) if got == kind => return Ok(items),
            Ok((got, _)) => tracing::debug!("ignoring a {} list", got.label()),
            Err(err) => tracing::debug!("display list reply not understood: {err}"),
        }
    }
    Err(BandError::DisplayList(format!(
        "the band did not report its {}",
        kind.label()
    )))
}

/// One entry, ready for a UI to show and reorder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub id: String,
    /// The band's name for a built-in screen, or an app's name, or its id
    /// when this Mac knows no name for it.
    pub name: String,
    /// True when the band has no name for it, which means it is an app.
    pub is_app: bool,
    /// An app installed from here: this Mac knows its name, from the
    /// manifest it was installed from, or as one of this project's own
    /// apps. The window tags it custom.
    pub installed_here: bool,
    pub shown: bool,
    pub position: u8,
}

/// One list, ready for a UI.
pub async fn read_list(session: &mut Session, kind: ListKind) -> Result<Vec<Entry>, BandError> {
    let items = fetch(session, kind).await?;
    // Read the app names once, not once a row: the learned half is a file.
    Ok(entries(kind, &items, &crate::app_names::all()))
}

/// A list's entries, named: `apps` is every app name this Mac knows, from
/// [`crate::app_names::all`]. Shown entries come first, in their order, then
/// the rest by name. A person reading the list cares about what the band
/// shows.
fn entries(
    kind: ListKind,
    items: &[DisplayItem],
    apps: &std::collections::BTreeMap<String, String>,
) -> Vec<Entry> {
    let mut entries: Vec<Entry> = items
        .iter()
        .map(|item| {
            let known = menu_names::name_for(table_for(kind), &item.id);
            let app = match known {
                Some(_) => None,
                None => crate::app_names::in_map(apps, &item.id),
            };
            Entry {
                id: item.id.clone(),
                installed_here: app.is_some(),
                name: known
                    .map(str::to_string)
                    .or(app)
                    .unwrap_or_else(|| item.id.clone()),
                is_app: known.is_none(),
                shown: item.shown(),
                position: item.position,
            }
        })
        .collect();
    entries.sort_by(|a, b| {
        b.shown
            .cmp(&a.shown)
            .then(a.position.cmp(&b.position))
            .then(a.name.cmp(&b.name))
    });
    entries
}

/// Every list, with each entry's section and position.
pub async fn read_all(session: &mut Session) -> Result<Vec<String>, BandError> {
    let names = crate::app_names::all();
    let mut lines = Vec::new();
    for kind in [ListKind::Menu, ListKind::Shortcuts, ListKind::ControlCenter] {
        let items = fetch(session, kind).await?;
        lines.push(format!("{} ({} entries)", kind.label(), items.len()));

        let mut sorted: Vec<&DisplayItem> = items.iter().collect();
        sorted.sort_by_key(|i| (i.section.label(), i.position));
        for item in sorted {
            let version = item.version().map(|v| format!("  v{v}")).unwrap_or_default();
            lines.push(format!(
                "  {:<7} {:>2}  {}{}",
                item.section.label(),
                item.position,
                describe(kind, item, &names),
                version
            ));
        }
        lines.push(String::new());
    }
    Ok(lines)
}

/// Answer one request about the screens: put the app menu in order with
/// `tidy`, read every list when there is nothing to change, or change the
/// list `kind` names. `set` and `hide` are as [`arrange`] takes them.
pub async fn serve(
    session: &mut Session,
    kind: ListKind,
    set: Option<&[String]>,
    hide: &[String],
    tidy: bool,
) -> Result<Vec<String>, BandError> {
    if tidy {
        tidy_menu(session, None).await
    } else if set.is_none() && hide.is_empty() {
        read_all(session).await
    } else {
        arrange(session, kind, set, hide).await
    }
}

/// Change what one list shows, then report what it shows now.
///
/// The band is asked for the current list first. Writing a list built from
/// anything else risks dropping entries, because each one carries flags and
/// strings this program does not interpret.
pub async fn arrange(
    session: &mut Session,
    kind: ListKind,
    set: Option<&[String]>,
    hide: &[String],
) -> Result<Vec<String>, BandError> {
    let current = fetch(session, kind).await?;
    let wanted = wanted_order(kind, &current, set, hide);
    write_and_read_back(session, kind, &current, &wanted).await
}

/// What a list shows after a change: exactly `set`, in its order, when it is
/// given, even when it holds nothing. With no `set`, what the band shows now,
/// in its order, less `hide`.
fn wanted_order(
    kind: ListKind,
    current: &[DisplayItem],
    set: Option<&[String]>,
    hide: &[String],
) -> Vec<String> {
    if let Some(set) = set {
        return set.iter().map(|name| resolve(kind, name)).collect();
    }
    let drop: Vec<String> = hide.iter().map(|name| resolve(kind, name)).collect();
    let mut shown: Vec<&DisplayItem> = current.iter().filter(|i| i.shown()).collect();
    shown.sort_by_key(|i| i.position);
    shown
        .into_iter()
        .map(|i| i.id.clone())
        .filter(|id| !drop.iter().any(|d| d.eq_ignore_ascii_case(id)))
        .collect()
}

/// Rebuild the app menu as every installed app, then settings, with the
/// band's own screens out of view.
///
/// Only the command line asks for this: `screens --tidy`, and its own install
/// when no window holds the band, which passes the app it installed as
/// `newest` to go first. The window never rebuilds the menu: New app on top
/// lifts the app just installed and leaves the rest as the person arranged
/// it (D10). See [`lift_to_top`].
pub async fn tidy_menu(
    session: &mut Session,
    newest: Option<&str>,
) -> Result<Vec<String>, BandError> {
    let kind = ListKind::Menu;
    let current = fetch(session, kind).await?;
    let order = tidy_order(&current, newest);
    write_and_read_back(session, kind, &current, &order).await
}

/// The menu a tidy writes, from the menu the band holds: every installed
/// app, `newest` first, then settings.
fn tidy_order(current: &[DisplayItem], newest: Option<&str>) -> Vec<String> {
    let settings = menu_names::id_for(menu_names::MENU, "settings").unwrap_or("00000013");
    // An installed app is the one thing the band has no name for.
    let is_app = |id: &str| menu_names::name_for(menu_names::MENU, id).is_none();
    shortcuts::apps_then_settings(current, is_app, settings, newest)
}

/// Put `newest`, the app an install just put on the band, first in the app
/// menu, and leave every other screen where it is (D10). The band drops a
/// new app into the menu wherever it likes.
///
/// A menu that shows the app first already, or does not list it at all, is
/// left alone, and nothing is said.
pub async fn lift_to_top(session: &mut Session, newest: &str) -> Result<Vec<String>, BandError> {
    let kind = ListKind::Menu;
    let current = fetch(session, kind).await?;
    let Some(order) = lift_order(&current, newest) else {
        return Ok(Vec::new());
    };
    write_and_read_back(session, kind, &current, &order).await
}

/// The menu a lift writes: `newest` first, then the screens the band shows
/// now, in their order. None when there is nothing to move: the menu shows
/// `newest` first already, or does not list it.
pub(crate) fn lift_order(current: &[DisplayItem], newest: &str) -> Option<Vec<String>> {
    let new = current
        .iter()
        .find(|item| item.id.eq_ignore_ascii_case(newest))?;
    let mut shown: Vec<&DisplayItem> = current.iter().filter(|item| item.shown()).collect();
    shown.sort_by_key(|item| item.position);
    if shown.first().is_some_and(|first| first.id == new.id) {
        return None;
    }
    let rest = shown
        .into_iter()
        .filter(|item| item.id != new.id)
        .map(|item| item.id.clone());
    Some(std::iter::once(new.id.clone()).chain(rest).collect())
}

async fn write_and_read_back(
    session: &mut Session,
    kind: ListKind,
    current: &[DisplayItem],
    wanted: &[String],
) -> Result<Vec<String>, BandError> {
    let command = shortcuts::encode_set(kind, current, wanted)?;
    session.send(shortcuts::ENDPOINT, &command, true).await?;

    // Read back rather than trust the write.
    let after = fetch(session, kind).await?;
    Ok(shown_lines(kind, &after))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_name_maps_to_its_kind() {
        assert_eq!(kind_from_name("menu").unwrap(), ListKind::Menu);
        assert_eq!(kind_from_name("shortcuts").unwrap(), ListKind::Shortcuts);
        assert_eq!(kind_from_name("control").unwrap(), ListKind::ControlCenter);
        assert!(kind_from_name("nope").is_err());
    }

    #[test]
    fn a_name_resolves_to_an_id_and_an_id_stays_itself() {
        assert_eq!(resolve(ListKind::Menu, "music"), "0000000B");
        assert_eq!(resolve(ListKind::Shortcuts, "music"), "00000004");
        assert_eq!(resolve(ListKind::Menu, "58e537b5"), "58E537B5");
    }

    /// An installed app has no name, and must still be readable in a list.
    #[test]
    fn an_app_is_labelled_as_one() {
        let app = DisplayItem {
            id: "58E537B5".to_string(),
            section: shortcuts::Section::Main,
            position: 0,
            flags: 0,
            extra: vec![],
        };
        // The one record of names is app-names.toml. An app it names shows
        // that name. Nothing else names an app, not even this project's own.
        let names =
            std::collections::BTreeMap::from([("58E537B5".to_string(), "BandBridge".to_string())]);
        assert_eq!(describe(ListKind::Menu, &app, &names), "BandBridge (app)");
        let nothing = std::collections::BTreeMap::new();
        assert_eq!(describe(ListKind::Menu, &app, &nothing), "58E537B5 (app)");

        let stranger = DisplayItem {
            id: "DEADBEEF".to_string(),
            ..app.clone()
        };
        assert_eq!(
            describe(ListKind::Menu, &stranger, &names),
            "DEADBEEF (app)"
        );

        let known = DisplayItem {
            id: "0000000B".to_string(),
            ..app.clone()
        };
        assert_eq!(describe(ListKind::Menu, &known, &names), "music (0000000B)");
    }

    #[test]
    fn an_empty_list_says_so() {
        let lines = shown_lines(ListKind::Shortcuts, &[]);
        assert_eq!(lines[1], "  nothing");
    }

    fn menu_item(id: &str, section: shortcuts::Section, position: u8) -> DisplayItem {
        DisplayItem {
            id: id.to_string(),
            section,
            position,
            flags: 0,
            extra: vec![],
        }
    }

    /// A lift puts the new app first, and every other shown screen keeps its
    /// order. A screen out of view stays out of view.
    #[test]
    fn a_lift_moves_the_new_app_and_nothing_else() {
        use shortcuts::Section::{Disabled, Main};
        let menu = [
            menu_item("0000000B", Main, 1),
            menu_item("58E537B5", Main, 0),
            menu_item("10BD37D4", Main, 2),
            menu_item("00000013", Main, 3),
            menu_item("58E538D5", Disabled, 0),
        ];
        assert_eq!(
            lift_order(&menu, "10bd37d4"),
            Some(vec![
                "10BD37D4".to_string(),
                "58E537B5".to_string(),
                "0000000B".to_string(),
                "00000013".to_string(),
            ])
        );
    }

    /// A new app the band keeps out of view comes into view, first.
    #[test]
    fn a_lift_brings_a_new_app_out_of_view_to_the_top() {
        use shortcuts::Section::{Disabled, Main};
        let menu = [
            menu_item("58E537B5", Main, 0),
            menu_item("00000013", Main, 1),
            menu_item("10BD37D4", Disabled, 0),
        ];
        assert_eq!(
            lift_order(&menu, "10BD37D4"),
            Some(vec![
                "10BD37D4".to_string(),
                "58E537B5".to_string(),
                "00000013".to_string(),
            ])
        );
    }

    /// Nothing to write: the app is first already, or the menu does not list
    /// it at all.
    #[test]
    fn a_lift_with_nothing_to_move_writes_nothing() {
        use shortcuts::Section::Main;
        let menu = [
            menu_item("10BD37D4", Main, 0),
            menu_item("00000013", Main, 1),
        ];
        assert_eq!(lift_order(&menu, "10BD37D4"), None);
        assert_eq!(lift_order(&menu, "58E537B5"), None);
    }

    /// Shortcuts as the band may hold them: two shown, out of order in the
    /// reply, and one in the pool.
    fn some_shortcuts() -> Vec<DisplayItem> {
        let item = |id: &str, section, position| DisplayItem {
            id: id.to_string(),
            section,
            position,
            flags: 0,
            extra: vec![],
        };
        vec![
            item("00000004", shortcuts::Section::Main, 1),
            item("00000001", shortcuts::Section::Main, 0),
            item("00000002", shortcuts::Section::More, 0),
        ]
    }

    /// A list of nothing is a list: the band then shows no shortcut. It is
    /// not the same as no list at all, which keeps what the band shows.
    #[test]
    fn a_list_of_nothing_shows_nothing() {
        let band = some_shortcuts();
        assert!(wanted_order(ListKind::Shortcuts, &band, Some(&[]), &[]).is_empty());
        assert_eq!(
            wanted_order(ListKind::Shortcuts, &band, None, &[]),
            ["00000001", "00000004"]
        );
    }

    #[test]
    fn a_list_to_show_is_shown_in_its_order() {
        let set = ["music".to_string(), "00000001".to_string()];
        assert_eq!(
            wanted_order(ListKind::Shortcuts, &some_shortcuts(), Some(&set), &[]),
            ["00000004", "00000001"]
        );
    }

    #[test]
    fn a_hide_leaves_the_rest_in_order() {
        let hide = ["00000001".to_string()];
        assert_eq!(
            wanted_order(ListKind::Shortcuts, &some_shortcuts(), None, &hide),
            ["00000004"]
        );
    }

    /// The window's list names each entry: a built-in screen by the band's
    /// name for it, and an app by the name this Mac knows, or by its id. An
    /// app this Mac can name is one installed from here, which the window
    /// tags custom. What the band shows comes first, in its order.
    #[test]
    fn a_list_names_each_entry_and_marks_the_apps_installed_here() {
        let item = |id: &str, section, position| DisplayItem {
            id: id.to_string(),
            section,
            position,
            flags: 0,
            extra: vec![],
        };
        let band = [
            item("00000002", shortcuts::Section::Disabled, 0),
            item("DEADBEEF", shortcuts::Section::Main, 1),
            item("58E537B5", shortcuts::Section::Main, 0),
            item("00000013", shortcuts::Section::Main, 2),
        ];
        let names =
            std::collections::BTreeMap::from([("58E537B5".to_string(), "BandBridge".to_string())]);
        let listed = entries(ListKind::Menu, &band, &names);
        let json = serde_json::to_value(&listed).unwrap();
        assert_eq!(
            json,
            serde_json::json!([
                {"id": "58E537B5", "name": "BandBridge", "is_app": true, "installed_here": true, "shown": true, "position": 0},
                {"id": "DEADBEEF", "name": "DEADBEEF", "is_app": true, "installed_here": false, "shown": true, "position": 1},
                {"id": "00000013", "name": "settings", "is_app": false, "installed_here": false, "shown": true, "position": 2},
                {"id": "00000002", "name": "hr", "is_app": false, "installed_here": false, "shown": false, "position": 0},
            ])
        );
    }
}
