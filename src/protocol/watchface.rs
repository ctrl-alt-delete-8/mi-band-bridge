//! The watch faces the band holds, and the one it is wearing.
//!
//! Endpoint `0x0023`, encrypted. Gadgetbridge calls this
//! ZeppOsWatchfaceService.
//!
//! This project does not change watch faces. It reads them for one reason: the
//! app list reports a watch face and an app with the same kind of id, so
//! without this list a removal cannot tell a program from the face on the
//! screen.

/// Where watch faces live.
pub const ENDPOINT: u16 = 0x0023;

const LIST_GET: u8 = 0x05;
const LIST_RET: u8 = 0x06;
const CURRENT_GET: u8 = 0x09;
const CURRENT_RET: u8 = 0x0a;

/// Ask which watch faces are installed.
pub fn list_request() -> Vec<u8> {
    vec![LIST_GET]
}

/// Ask which one the band is wearing.
pub fn current_request() -> Vec<u8> {
    vec![CURRENT_GET]
}

/// The ids in a list reply, or None when this is not one.
///
/// The reply is the command, a status byte, a count, then that many ids of
/// four bytes each.
pub fn parse_list(payload: &[u8]) -> Option<Vec<String>> {
    if payload.first() != Some(&LIST_RET) {
        return None;
    }
    let count = *payload.get(2)? as usize;
    Some(
        (0..count)
            .filter_map(|i| {
                let at = 3 + i * 4;
                let four = payload.get(at..at + 4)?;
                Some(format!(
                    "{:08X}",
                    u32::from_le_bytes([four[0], four[1], four[2], four[3]])
                ))
            })
            .collect(),
    )
}

/// The id of the face the band is wearing, or None when this is not that reply.
pub fn parse_current(payload: &[u8]) -> Option<String> {
    if payload.first() != Some(&CURRENT_RET) {
        return None;
    }
    let four = payload.get(1..5)?;
    Some(format!(
        "{:08X}",
        u32::from_le_bytes([four[0], four[1], four[2], four[3]])
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The replies this band sent, byte for byte.
    #[test]
    fn the_bands_own_replies_parse() {
        let list = hex::decode("06010131ff0000").unwrap();
        assert_eq!(parse_list(&list), Some(vec!["0000FF31".to_string()]));

        let current = hex::decode("0a31ff0000").unwrap();
        assert_eq!(parse_current(&current), Some("0000FF31".to_string()));
    }

    #[test]
    fn another_reply_is_not_mistaken_for_one_of_these() {
        assert!(parse_list(&[0x0a, 0x31, 0xff, 0, 0]).is_none());
        assert!(parse_current(&[0x06, 0x01, 0x01]).is_none());
    }

    #[test]
    fn a_band_with_several_faces_lists_them_all() {
        // count 2, then two ids.
        let reply = hex::decode("060102382d0000102d0000").unwrap();
        assert_eq!(
            parse_list(&reply),
            Some(vec!["00002D38".to_string(), "00002D10".to_string()])
        );
    }
}
