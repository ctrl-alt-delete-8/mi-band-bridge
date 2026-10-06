//! The band's battery.
//!
//! The endpoint and the two command bytes come from Gadgetbridge's
//! ZeppOsBatteryService. The band answers a one byte request, and sends the
//! same reply on its own when the level changes, so a host that asks once
//! keeps hearing about it.

/// Where battery messages go.
pub const ENDPOINT: u16 = 0x0029;

/// This endpoint takes its messages unencrypted on this band.
///
/// Gadgetbridge builds its battery service encrypted, then overrides that from
/// the service list the band sends on endpoint 0x0000. The Band 7 lists
/// `0x0029 encrypted=0`, and it ignores an encrypted request, so send it plain.
/// See examples/services_probe.rs for the list.
pub const ENCRYPTED: bool = false;

/// Ask for the battery.
const CMD_REQUEST: u8 = 0x03;

/// The band's answer, and its own report of a change.
const CMD_REPLY: u8 = 0x04;

/// What the band says about its battery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    /// 0 to 100.
    pub percent: u8,
    pub charging: bool,
}

impl std::fmt::Display for Battery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}%", self.percent)?;
        if self.charging {
            f.write_str(", charging")?;
        }
        Ok(())
    }
}

/// The request bytes. Send them with ENCRYPTED.
pub fn request() -> Vec<u8> {
    vec![CMD_REQUEST]
}

/// The battery in a reply, or None when the payload is not one.
///
/// After the command byte the band sends one byte nothing here reads, then the
/// level, then the state, where 1 is charging. The rest is the last charge
/// time, which this program does not show.
pub fn parse_reply(payload: &[u8]) -> Option<Battery> {
    if payload.first() != Some(&CMD_REPLY) {
        return None;
    }
    let percent = *payload.get(2)?;
    let state = *payload.get(3)?;
    // A level above 100 is not a level. Refusing it keeps a garbled reply out
    // of the window, where the next reply will hold the real one.
    if percent > 100 {
        return None;
    }
    Some(Battery {
        percent,
        charging: state == 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reply as the band sends it: 21 bytes, level and state near the front,
    /// last charge time after them.
    fn reply(percent: u8, state: u8) -> Vec<u8> {
        let mut payload = vec![CMD_REPLY, 0x0f, percent, state];
        payload.extend_from_slice(&[0xe0, 0x07, 0x0b, 0x1a, 0x12, 0x23, 0x2c, 0x04]);
        payload.extend_from_slice(&[0xe0, 0x07, 0x0b, 0x1a, 0x17, 0x2b, 0x3b, 0x04, 0x64]);
        payload
    }

    #[test]
    fn a_reply_carries_the_level_and_the_state() {
        assert_eq!(reply(48, 0).len(), 21);
        assert_eq!(
            parse_reply(&reply(48, 0)),
            Some(Battery { percent: 48, charging: false })
        );
        assert_eq!(
            parse_reply(&reply(100, 1)),
            Some(Battery { percent: 100, charging: true })
        );
    }

    #[test]
    fn it_reads_as_a_person_would_say_it() {
        assert_eq!(Battery { percent: 48, charging: false }.to_string(), "48%");
        assert_eq!(Battery { percent: 48, charging: true }.to_string(), "48%, charging");
    }

    /// Our own request shares the endpoint with the band's answer, and the two
    /// are told apart by the command byte.
    #[test]
    fn nothing_else_reads_as_a_reply() {
        assert_eq!(parse_reply(&request()), None);
        assert_eq!(parse_reply(&[]), None);
        assert_eq!(parse_reply(&[CMD_REPLY, 0x0f]), None, "too short to hold a level");
        assert_eq!(parse_reply(&reply(101, 0)), None, "not a level");
    }
}
