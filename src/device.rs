//! What the band says about itself, from the standard GATT services.
//!
//! macOS hides a Bluetooth address: CoreBluetooth gives each host its own UUID
//! for a peripheral and reports the address as zeros. The address is still
//! reachable, because the band publishes it in the System ID characteristic of
//! the Device Information service, which every Bluetooth device may carry.

use btleplug::api::Peripheral as _;
use btleplug::platform::Peripheral;
use uuid::{Uuid, uuid};

/// Device Information: System ID, which holds the address.
const SYSTEM_ID: Uuid = uuid!("00002a23-0000-1000-8000-00805f9b34fb");
/// Device Information: serial number.
const SERIAL: Uuid = uuid!("00002a25-0000-1000-8000-00805f9b34fb");
/// Device Information: hardware revision.
const HARDWARE: Uuid = uuid!("00002a27-0000-1000-8000-00805f9b34fb");
/// Device Information: software revision, which is the band's firmware.
const FIRMWARE: Uuid = uuid!("00002a28-0000-1000-8000-00805f9b34fb");

/// What one band says about itself. Every field is optional, because a band
/// that does not answer must not stop a connection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// The Bluetooth address, as `00:00:5E:00:53:01`.
    pub mac: Option<String>,
    pub serial: Option<String>,
    pub hardware: Option<String>,
    pub firmware: Option<String>,
}

/// The Bluetooth address inside a System ID, if the band built one from it.
///
/// The convention fills the middle with `FF FE`: the three address bytes of
/// the maker, then `FF FE`, then the three bytes of the device. A System ID
/// built some other way is not an address, so it is refused rather than
/// printed as one.
pub fn mac_from_system_id(value: &[u8]) -> Option<String> {
    if value.len() != 8 || value[3] != 0xff || value[4] != 0xfe {
        return None;
    }
    let bytes = [value[0], value[1], value[2], value[5], value[6], value[7]];
    Some(
        bytes
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(":"),
    )
}

/// Whether two addresses name the same band, whatever their case and
/// separators: `00-00-5e-00-53-01` is `00:00:5E:00:53:01`. Something that is
/// not an address matches nothing.
pub fn same_address(a: &str, b: &str) -> bool {
    fn digits(address: &str) -> Option<String> {
        let digits: String = address
            .trim()
            .chars()
            .filter(|c| !matches!(c, ':' | '-'))
            .map(|c| c.to_ascii_uppercase())
            .collect();
        (digits.len() == 12 && digits.chars().all(|c| c.is_ascii_hexdigit())).then_some(digits)
    }
    matches!((digits(a), digits(b)), (Some(a), Some(b)) if a == b)
}

/// Read what the band says about itself.
///
/// Best effort: a characteristic the band does not have, or will not read,
/// leaves its field empty. The link is already open at this point, and none of
/// this is worth failing a connection over.
pub async fn read(peripheral: &Peripheral) -> Facts {
    let mut facts = Facts::default();
    for characteristic in peripheral.characteristics() {
        let wanted = matches!(
            characteristic.uuid,
            SYSTEM_ID | SERIAL | HARDWARE | FIRMWARE
        );
        if !wanted {
            continue;
        }
        let Ok(value) = peripheral.read(&characteristic).await else {
            tracing::debug!("the band did not answer {}", characteristic.uuid);
            continue;
        };
        match characteristic.uuid {
            SYSTEM_ID => facts.mac = mac_from_system_id(&value),
            SERIAL => facts.serial = text(&value),
            HARDWARE => facts.hardware = text(&value),
            FIRMWARE => facts.firmware = text(&value),
            _ => {}
        }
    }
    facts
}

/// A characteristic's value as text, or None when it is empty or not text.
fn text(value: &[u8]) -> Option<String> {
    let text = String::from_utf8(value.to_vec()).ok()?;
    let text = text.trim_end_matches('\0').trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The System ID's layout, as measured on the band, with the example
    /// address 00:00:5E:00:53:01 in place of the band's own. The band's
    /// advertised name ends with the last two bytes, 5301 here.
    #[test]
    fn a_system_id_holds_the_address() {
        let value = [0x00, 0x00, 0x5e, 0xff, 0xfe, 0x00, 0x53, 0x01];
        assert_eq!(
            mac_from_system_id(&value),
            Some("00:00:5E:00:53:01".to_string())
        );
    }

    #[test]
    fn anything_else_is_not_an_address() {
        // The two markers in the middle are what say an address is in there.
        assert_eq!(mac_from_system_id(&[0x00, 0x00, 0x5e, 0, 0, 0x00, 0x53, 0x01]), None);
        assert_eq!(mac_from_system_id(&[0x00, 0x00, 0x5e, 0xff, 0xfe]), None);
        assert_eq!(mac_from_system_id(&[]), None);
    }

    #[test]
    fn an_address_is_the_same_whatever_its_spelling() {
        assert!(same_address("00:00:5E:00:53:01", "00:00:5e:00:53:01"));
        assert!(same_address("00:00:5E:00:53:01", " 00-00-5e-00-53-01 "));
        assert!(same_address("00:00:5E:00:53:01", "00005E005301"));
        assert!(!same_address("00:00:5E:00:53:01", "00:00:5E:00:53:02"));
        assert!(!same_address("00:00:5E:00:53:01", "00:00:5E:00:53"));
        assert!(!same_address("", ""), "nothing is not an address");
        assert!(!same_address("not an address", "not an address"));
    }

    #[test]
    fn text_stops_at_the_padding() {
        assert_eq!(text(b"2.2.0.1\0\0"), Some("2.2.0.1".to_string()));
        assert_eq!(text(b"  "), None);
        assert_eq!(text(&[0xff, 0xfe]), None, "not text at all");
    }
}
