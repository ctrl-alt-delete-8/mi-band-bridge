//! Reading the band's display lists: the main menu, the swipe shortcuts, and
//! the control center.
//!
//! These live on their own chunked endpoint, not on the firmware service. The
//! wire format follows Gadgetbridge's ZeppOsDisplayItemsService: each entry is
//! a null-terminated id written as a literal hex string, then its section, its
//! position within that section, and a flags byte that may pull in one or two
//! more strings.

use crate::error::BandError;

/// The chunked endpoint carrying the display lists.
pub const ENDPOINT: u16 = 0x0026;

pub const CMD_REQUEST: u8 = 0x03;
pub const CMD_RESPONSE: u8 = 0x04;
pub const CMD_CREATE: u8 = 0x05;

/// The official app writes at most this many shortcuts.
pub const MAX_SHORTCUTS: usize = 10;

/// Which list to ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    /// The app list reached from the watch face.
    Menu = 0x01,
    /// The screens reached by swiping left and right.
    Shortcuts = 0x02,
    /// The panel reached by swiping down.
    ControlCenter = 0x03,
}

impl ListKind {
    pub fn label(&self) -> &'static str {
        match self {
            ListKind::Menu => "menu",
            ListKind::Shortcuts => "shortcuts",
            ListKind::ControlCenter => "control center",
        }
    }
}

/// Where an entry sits. Anything not in `Main` is off the visible list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Main,
    More,
    Disabled,
    Unknown(u8),
}

impl Section {
    fn from_byte(value: u8) -> Self {
        match value {
            0x01 => Section::Main,
            0x02 => Section::More,
            0x03 => Section::Disabled,
            other => Section::Unknown(other),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Section::Main => "shown".to_string(),
            Section::More => "more".to_string(),
            Section::Disabled => "hidden".to_string(),
            Section::Unknown(value) => format!("section {value:#04x}"),
        }
    }
}

/// One entry in a display list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayItem {
    /// The band's id for the screen, an 8-character upper-case hex string. A
    /// third-party app appears here as its app id in hex.
    pub id: String,
    pub section: Section,
    pub position: u8,
    /// The flags byte, kept verbatim. Writing a different value changes what
    /// the band thinks the entry is.
    pub flags: u8,
    /// The strings the flags byte pulls in, kept verbatim and in order so a
    /// list can be written back without losing them.
    pub extra: Vec<String>,
}

impl DisplayItem {
    /// Whether this entry is one the band shows.
    pub fn shown(&self) -> bool {
        self.section == Section::Main
    }

    /// The version string, when the entry carries one.
    pub fn version(&self) -> Option<&str> {
        self.extra.first().map(String::as_str)
    }
}

/// Ask the band for one list.
pub fn request(kind: ListKind) -> Vec<u8> {
    vec![CMD_REQUEST, kind as u8]
}

/// Read a null-terminated string, returning it and the index just past the nul.
fn read_string(bytes: &[u8], from: usize) -> Option<(String, usize)> {
    let end = bytes[from..].iter().position(|b| *b == 0)? + from;
    let text = String::from_utf8_lossy(&bytes[from..end]).into_owned();
    Some((text, end + 1))
}

/// Parse a display-list reply.
pub fn parse_response(payload: &[u8]) -> Result<(ListKind, Vec<DisplayItem>), BandError> {
    if payload.len() < 4 || payload[0] != CMD_RESPONSE {
        return Err(BandError::DisplayList("not a display list reply".to_string()));
    }

    let kind = match payload[1] {
        0x01 => ListKind::Menu,
        0x02 => ListKind::Shortcuts,
        0x03 => ListKind::ControlCenter,
        other => {
            return Err(BandError::DisplayList(format!(
                "unknown display list type {other:#04x}"
            )));
        }
    };

    let count = u16::from_le_bytes([payload[2], payload[3]]) as usize;
    let mut items = Vec::with_capacity(count);
    let mut at = 4;

    for _ in 0..count {
        let Some((id, next)) = read_string(payload, at) else {
            return Err(BandError::DisplayList("display list ended mid-entry".to_string()));
        };
        at = next;

        if at + 2 >= payload.len() {
            return Err(BandError::DisplayList("display list ended mid-entry".to_string()));
        }
        let section = Section::from_byte(payload[at]);
        let position = payload[at + 1];
        let flags = payload[at + 2];
        at += 3;

        // Bits 1 and 2 each pull in a trailing string. Which is which is not
        // known. Keep them verbatim so the list can be written back unchanged.
        let mut extra = Vec::new();
        if flags & 0b010 != 0 || flags & 0b100 != 0 {
            let Some((text, next)) = read_string(payload, at) else {
                return Err(BandError::DisplayList("display list ended mid-entry".to_string()));
            };
            extra.push(text);
            at = next;

            if flags & 0b010 != 0 && flags & 0b100 != 0 {
                let Some((text, next)) = read_string(payload, at) else {
                    return Err(BandError::DisplayList(
                        "display list ended mid-entry".to_string(),
                    ));
                };
                extra.push(text);
                at = next;
            }
        }

        items.push(DisplayItem {
            id,
            section,
            position,
            flags,
            extra,
        });
    }

    Ok((kind, items))
}

/// Build the command that replaces a display list.
///
/// `shown` names the entries to show, in the order they should appear. Every
/// other entry in `current` is written back as disabled, keeping its flags and
/// strings, so nothing is lost from lists this program does not understand.
///
/// Entries not named in `shown` keep the section and position the band gave
/// them, so a list that has a "more" pool comes back with that pool intact.
///
/// An id absent from `current` is refused. Installed apps are listed only in
/// the menu, and the band discards a shortcuts write that names one: shortcut
/// cards need API level 2.0, and this band is 1.0.
pub fn encode_set(
    kind: ListKind,
    current: &[DisplayItem],
    shown: &[String],
) -> Result<Vec<u8>, BandError> {
    if kind == ListKind::Shortcuts && shown.len() > MAX_SHORTCUTS {
        return Err(BandError::DisplayList(format!(
            "the band takes at most {MAX_SHORTCUTS} shortcuts, {} given",
            shown.len()
        )));
    }

    let find = |wanted: &str| current.iter().find(|item| item.id.eq_ignore_ascii_case(wanted));

    for id in shown {
        if find(id).is_none() {
            return Err(BandError::DisplayList(format!(
                "the {} list has no entry {id}. Installed apps cannot be \
                 shortcuts on this band: shortcut cards need API level 2.0.",
                kind.label()
            )));
        }
    }

    let mut out = vec![CMD_CREATE, kind as u8];
    out.extend_from_slice(&(current.len() as u16).to_le_bytes());

    let write_item = |item: &DisplayItem, section: Section, position: u8, out: &mut Vec<u8>| {
        out.extend_from_slice(item.id.as_bytes());
        out.push(0);
        out.push(match section {
            Section::Main => 0x01,
            Section::More => 0x02,
            Section::Disabled => 0x03,
            Section::Unknown(value) => value,
        });
        out.push(position);
        out.push(item.flags);
        for text in &item.extra {
            out.extend_from_slice(text.as_bytes());
            out.push(0);
        }
    };

    for (position, id) in shown.iter().enumerate() {
        let item = find(id).expect("checked above");
        write_item(item, Section::Main, position as u8, &mut out);
    }

    // Everything else keeps the place the band gave it. Forcing these to
    // disabled would empty the "more" pool the band offers.
    for item in current {
        if shown.iter().any(|id| id.eq_ignore_ascii_case(&item.id)) {
            continue;
        }
        let section = if item.section == Section::Main {
            Section::Disabled
        } else {
            item.section
        };
        write_item(item, section, item.position, &mut out);
    }

    Ok(out)
}

/// The menu a tidy writes: the newest app first, then the rest of the apps,
/// then settings. Only the command line tidies. The window moves just the
/// app it installed, and leaves the rest as the person arranged it (D10).
///
/// The band drops a freshly installed app wherever it likes, so a tidy
/// rebuilds the list from scratch rather than nudge it. `newest` is the app
/// just installed, and it goes to the top: you install an app in order to
/// press it, and the band's menu is one column you scroll with a wrist.
///
/// Settings cannot be removed, so it is kept and pushed to the end. `is_app`
/// tells an installed app from a built-in screen: the band has no name for an
/// app id.
pub fn apps_then_settings(
    current: &[DisplayItem],
    is_app: impl Fn(&str) -> bool,
    settings_id: &str,
    newest: Option<&str>,
) -> Vec<String> {
    let mut order: Vec<String> = current
        .iter()
        .filter(|item| is_app(&item.id))
        .map(|item| item.id.clone())
        .collect();
    // The band may report it anywhere in the list, so move it rather than
    // assume where it is.
    if let Some(newest) = newest
        && let Some(at) = order.iter().position(|id| id.eq_ignore_ascii_case(newest))
    {
        let app = order.remove(at);
        order.insert(0, app);
    }

    if let Some(settings) = current
        .iter()
        .find(|item| item.id.eq_ignore_ascii_case(settings_id))
    {
        order.push(settings.id.clone());
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;


    fn item(id: &str, section: Section, position: u8) -> DisplayItem {
        DisplayItem {
            id: id.to_string(),
            section,
            position,
            flags: 0,
            extra: vec![],
        }
    }

    #[test]
    fn writes_the_named_entries_first_and_disables_the_rest() {
        let current = vec![
            item("0000000A", Section::Main, 0),
            item("0000000B", Section::More, 0),
            item("0000000C", Section::More, 1),
        ];
        let out = encode_set(
            ListKind::Shortcuts,
            &current,
            &["0000000C".to_string(), "0000000A".to_string()],
        )
        .expect("encodes");

        assert_eq!(&out[..4], &[CMD_CREATE, 0x02, 0x03, 0x00]);
        // shown, in the order given, positions from zero
        assert_eq!(&out[4..13], b"0000000C\0");
        assert_eq!(&out[13..16], &[0x01, 0x00, 0x00]);
        assert_eq!(&out[16..25], b"0000000A\0");
        assert_eq!(&out[25..28], &[0x01, 0x01, 0x00]);
        // the leftover keeps the section and position the band gave it
        assert_eq!(&out[28..37], b"0000000B\0");
        assert_eq!(&out[37..40], &[0x02, 0x00, 0x00]);
    }

    /// Flags and their strings must survive a write, or entries this program
    /// does not understand come back corrupted.
    #[test]
    fn a_write_keeps_flags_and_trailing_strings() {
        let current = vec![DisplayItem {
            id: "58E537B5".to_string(),
            section: Section::Main,
            position: 0,
            flags: 0b010,
            extra: vec!["1.0.9".to_string()],
        }];
        let out = encode_set(ListKind::Menu, &current, &["58E537B5".to_string()]).expect("encodes");
        assert_eq!(&out[4..13], b"58E537B5\0");
        assert_eq!(&out[13..16], &[0x01, 0x00, 0b010]);
        assert_eq!(&out[16..22], b"1.0.9\0");
    }

    /// An installed app is listed in the menu, never in the shortcuts, and the
    /// band discards a write that names one. Refuse it here with a reason.
    #[test]
    fn refuses_an_id_the_list_does_not_have() {
        let current = vec![item("0000000A", Section::Main, 0)];
        let err = encode_set(ListKind::Shortcuts, &current, &["58E537B5".to_string()])
            .expect_err("refused");
        assert!(err.to_string().contains("API level 2.0"));
    }

    /// A shown entry that is dropped becomes disabled, but one already parked
    /// in the "more" pool stays there.
    #[test]
    fn a_write_keeps_the_more_pool() {
        let current = vec![
            item("0000000A", Section::Main, 0),
            item("0000000B", Section::More, 3),
        ];
        let out = encode_set(ListKind::Shortcuts, &current, &[]).expect("encodes");
        assert_eq!(&out[4..13], b"0000000A\0");
        assert_eq!(&out[13..16], &[0x03, 0x00, 0x00]);
        assert_eq!(&out[16..25], b"0000000B\0");
        assert_eq!(&out[25..28], &[0x02, 0x03, 0x00]);
    }

    #[test]
    fn refuses_more_shortcuts_than_the_band_takes() {
        let current: Vec<DisplayItem> = (0..12)
            .map(|i| item(&format!("{i:08X}"), Section::Main, i as u8))
            .collect();
        let all: Vec<String> = current.iter().map(|i| i.id.clone()).collect();
        assert!(encode_set(ListKind::Shortcuts, &current, &all).is_err());
        // the menu has no such cap
        assert!(encode_set(ListKind::Menu, &current, &all).is_ok());
    }


    #[test]
    fn menu_order_is_every_app_then_settings() {
        let current = vec![
            item("00000013", Section::Main, 0),
            item("58E537B5", Section::Main, 1),
            item("0000000B", Section::Main, 2),
            item("58E538D5", Section::Disabled, 0),
        ];
        let is_app = |id: &str| id.starts_with("58E5");
        let order = apps_then_settings(&current, is_app, "00000013", None);
        assert_eq!(order, vec!["58E537B5", "58E538D5", "00000013"]);
    }

    /// You install an app in order to press it, and the band's menu is one
    /// column scrolled with a wrist, so the new one goes to the top wherever
    /// the band reported it.
    #[test]
    fn the_newest_app_goes_first() {
        let current = vec![
            item("58E537B5", Section::Main, 0),
            item("0000000B", Section::Main, 1),
            item("10BD37D4", Section::Main, 2),
            item("00000013", Section::Main, 3),
            item("58E538D5", Section::Disabled, 0),
        ];
        let is_app = |id: &str| id.starts_with("58E5") || id.starts_with("10BD");
        let order = apps_then_settings(&current, is_app, "00000013", Some("10bd37d4"));
        assert_eq!(order, vec!["10BD37D4", "58E537B5", "58E538D5", "00000013"]);
    }

    /// An id the band does not list leaves the order alone rather than
    /// producing an app that is not there.
    #[test]
    fn an_unknown_newest_changes_nothing() {
        let current = vec![
            item("58E537B5", Section::Main, 0),
            item("00000013", Section::Main, 1),
        ];
        let is_app = |id: &str| id.starts_with("58E5");
        let order = apps_then_settings(&current, is_app, "00000013", Some("DEADBEEF"));
        assert_eq!(order, vec!["58E537B5", "00000013"]);
    }

    /// An app the band parked out of the menu still belongs in it, and music
    /// stays out however the band left it.
    #[test]
    fn menu_order_drops_built_in_screens() {
        let current = vec![
            item("0000000B", Section::Main, 0),
            item("00000013", Section::Main, 1),
        ];
        let order = apps_then_settings(&current, |id: &str| id.starts_with("58E5"), "00000013", None);
        assert_eq!(order, vec!["00000013"]);
    }

    #[test]
    fn request_names_the_list() {
        assert_eq!(request(ListKind::Shortcuts), vec![CMD_REQUEST, 0x02]);
        assert_eq!(request(ListKind::Menu), vec![CMD_REQUEST, 0x01]);
    }

    #[test]
    fn parses_entries_without_flags() {
        // two entries: "1e" shown at 0, "2f" hidden at 1
        let payload = [
            CMD_RESPONSE, 0x02, 0x02, 0x00, b'1', b'e', 0x00, 0x01, 0x00, 0x00, b'2', b'f', 0x00,
            0x03, 0x01, 0x00,
        ];
        let (kind, items) = parse_response(&payload).expect("parses");
        assert_eq!(kind, ListKind::Shortcuts);
        assert_eq!(
            items,
            vec![
                DisplayItem {
                    id: "1e".to_string(),
                    section: Section::Main,
                    position: 0,
                    flags: 0,
                    extra: vec![],
                },
                DisplayItem {
                    id: "2f".to_string(),
                    section: Section::Disabled,
                    position: 1,
                    flags: 0,
                    extra: vec![],
                },
            ]
        );
    }

    /// A flagged entry carries a trailing string. Skipping it would desync
    /// every entry after it, which looks like corrupt ids rather than an error.
    #[test]
    fn parses_an_entry_that_carries_a_version() {
        let payload = [
            CMD_RESPONSE, 0x01, 0x02, 0x00, b'a', b'1', 0x00, 0x01, 0x00, 0x02, b'1', b'.', b'0',
            0x00, b'b', b'2', 0x00, 0x01, 0x01, 0x00,
        ];
        let (_, items) = parse_response(&payload).expect("parses");
        assert_eq!(items[0].version(), Some("1.0"));
        assert_eq!(items[1].id, "b2");
        assert_eq!(items[1].position, 1);
    }

    #[test]
    fn rejects_a_reply_that_is_not_a_display_list() {
        assert!(parse_response(&[0x01, 0x02, 0x00, 0x00]).is_err());
        assert!(parse_response(&[CMD_RESPONSE]).is_err());
    }

    #[test]
    fn rejects_a_truncated_entry() {
        let payload = [CMD_RESPONSE, 0x02, 0x01, 0x00, b'1', b'e', 0x00, 0x01];
        assert!(parse_response(&payload).is_err());
    }
}
