//! Listing and removing the apps installed on the band.
//!
//! Endpoint `0x00a0`, unencrypted. A custom app's own bytes arrive on the same
//! endpoint, and they do not collide because the first byte is a type tag:
//! `0x01` is a JavaScript message, `0x02` is app management, `0x03` is a
//! screenshot. A signal of this project's own starts at `0xbb`.
//!
//! Read from Gadgetbridge's `ZeppOsAppsService`, then confirmed against the
//! band. Gadgetbridge keeps its delete behind an experimental setting, so the
//! removal path here is held to the same caution: an id that the band did not
//! just list is refused.

/// Where app management lives: the same endpoint a band app sends on.
pub const ENDPOINT: u16 = 0x00a0;

/// The first byte: app management, rather than a page's own bytes.
pub const CMD_APPS: u8 = 0x02;
/// The second byte: which way the message goes.
pub const TO_BAND: u8 = 0x01;
pub const FROM_BAND: u8 = 0x00;
/// The third byte: which command.
pub const APPS_LIST: u8 = 0x01;
pub const APPS_DELETE: u8 = 0x03;
pub const APPS_DELETING: u8 = 0x04;

/// Every message keeps its first 16 bytes for the header, and starts its data
/// at byte 16.
const DATA_AT: usize = 16;

/// One app, as the band reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledApp {
    /// The id, in the upper case hex the band uses.
    pub id: String,
    /// The version string from the app's manifest, such as `1.0.11`.
    pub version: String,
}

impl InstalledApp {
    /// The id as a number, which is what a manifest holds.
    pub fn number(&self) -> Option<u32> {
        u32::from_str_radix(&self.id, 16).ok()
    }
}

/// Ask for the list of installed apps.
pub fn list_request() -> Vec<u8> {
    let mut out = vec![0u8; DATA_AT];
    out[0] = CMD_APPS;
    out[1] = TO_BAND;
    out[2] = APPS_LIST;
    out
}

/// Ask the band to remove one app.
pub fn delete_request(app_id: u32) -> Vec<u8> {
    let mut out = vec![0u8; DATA_AT + 4];
    out[0] = CMD_APPS;
    out[1] = TO_BAND;
    out[2] = APPS_DELETE;
    out[DATA_AT..].copy_from_slice(&app_id.to_le_bytes());
    out
}

/// Is this payload app management, rather than a signal from an app?
///
/// The event decoder asks this, so a reply to a request of ours never reads as
/// a button press.
pub fn is_management(payload: &[u8]) -> bool {
    matches!(payload.first(), Some(&CMD_APPS)) && payload.get(1) == Some(&FROM_BAND)
}

/// The apps in a list reply, or None when this is not one.
///
/// The band answers with one string: `<id>-<version>;<id>-<version>;`, in
/// upper case hex, ending at a zero byte.
pub fn parse_list(payload: &[u8]) -> Option<Vec<InstalledApp>> {
    if !is_management(payload) || payload.get(2) != Some(&APPS_LIST) {
        return None;
    }
    let text: Vec<u8> = payload
        .get(DATA_AT..)?
        .iter()
        .copied()
        .take_while(|b| *b != 0)
        .collect();
    let text = String::from_utf8_lossy(&text).into_owned();
    Some(
        text.split(';')
            .filter(|entry| !entry.is_empty())
            .filter_map(|entry| {
                let (id, version) = entry.split_once('-')?;
                Some(InstalledApp {
                    id: id.to_string(),
                    version: version.to_string(),
                })
            })
            .collect(),
    )
}

/// Does this payload answer a delete request?
pub fn is_delete_reply(payload: &[u8]) -> bool {
    is_management(payload) && matches!(payload.get(2), Some(&APPS_DELETE) | Some(&APPS_DELETING))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact bytes Gadgetbridge sends, which the band answered.
    #[test]
    fn a_list_request_is_sixteen_bytes_of_header() {
        let request = list_request();
        assert_eq!(hex::encode(&request), "02010100000000000000000000000000");
    }

    #[test]
    fn a_delete_request_carries_the_id_little_endian_at_sixteen() {
        // 1491417301 is BandArrows, 0x58e538d5.
        let request = delete_request(1_491_417_301);
        let expected = "02010300000000000000000000000000".to_string() + "d538e558";
        assert_eq!(hex::encode(&request), expected);
    }

    /// The reply this band actually sent, byte for byte.
    #[test]
    fn the_bands_own_reply_parses() {
        let reply = hex::decode(
            "02000100000000000000000000000000394536413333312d312e303b394536413333322d312e303b\
             35384535333742352d312e302e31313b35384535333844352d312e302e343b464633312d312e332e\
             303b31304244333744342d312e302e323b",
        )
        .unwrap();
        let apps = parse_list(&reply).expect("a list reply");
        assert_eq!(apps.len(), 6);
        assert_eq!(apps[2], InstalledApp { id: "58E537B5".into(), version: "1.0.11".into() });
        // The id is hex, and the number is what the manifest holds.
        assert_eq!(apps[2].number(), Some(1_491_417_013));
        assert_eq!(apps[3].number(), Some(1_491_417_301));
    }

    /// A press must never read as app management, and a reply must never read
    /// as a press.
    #[test]
    fn a_signal_and_a_reply_are_told_apart() {
        assert!(!is_management(&[0xbb, 0x01]));
        assert!(is_management(&[0x02, 0x00, 0x01]));
        // A request of ours, echoed back somehow, is not a reply.
        assert!(!is_management(&[0x02, 0x01, 0x01]));
        assert!(parse_list(&[0xbb, 0x04]).is_none());
    }

    #[test]
    fn an_empty_list_is_a_list_and_not_a_failure() {
        let mut reply = vec![0u8; DATA_AT + 1];
        reply[0] = CMD_APPS;
        reply[1] = FROM_BAND;
        reply[2] = APPS_LIST;
        assert_eq!(parse_list(&reply), Some(Vec::new()));
    }

    #[test]
    fn both_delete_answers_count() {
        assert!(is_delete_reply(&[CMD_APPS, FROM_BAND, APPS_DELETE]));
        assert!(is_delete_reply(&[CMD_APPS, FROM_BAND, APPS_DELETING]));
        assert!(!is_delete_reply(&[CMD_APPS, FROM_BAND, APPS_LIST]));
    }
}
