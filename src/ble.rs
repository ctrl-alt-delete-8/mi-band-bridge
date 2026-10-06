use crate::error::BandError;
use btleplug::api::{
    Central, CharPropFlags, Characteristic, Manager as _, Peripheral as _,
    RetrievePeripheralsOptions, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
#[cfg(target_os = "macos")]
use btleplug::platform::PeripheralId;
use std::time::Duration;
use uuid::{Uuid, uuid};

/// Zepp 2021 chunked transport, write side.
pub const CHAR_WRITE: Uuid = uuid!("00000016-0000-3512-2118-0009af100700");
/// Zepp 2021 chunked transport, notify side. Acknowledgements are written here too.
pub const CHAR_READ: Uuid = uuid!("00000017-0000-3512-2118-0009af100700");
/// The Huami/Zepp service holding the chunked transport. Used to ask macOS for
/// the band directly when it is connected, and therefore silent.
pub const SERVICE_ZEPP: Uuid = uuid!("0000fee0-0000-1000-8000-00805f9b34fb");

/// The name prefix the Band 7 advertises.
pub const BAND_NAME_HINT: &str = "Xiaomi Smart Band";

/// Firmware control: commands out, replies back by notification.
pub const CHAR_FW_CONTROL: Uuid = uuid!("00001531-0000-3512-2118-0009af100700");
/// Firmware payload stream.
pub const CHAR_FW_DATA: Uuid = uuid!("00001532-0000-3512-2118-0009af100700");

pub struct Chunked {
    pub write: Characteristic,
    pub read: Characteristic,
}

pub struct Firmware {
    pub control: Characteristic,
    pub data: Characteristic,
}

pub async fn adapter() -> Result<Adapter, BandError> {
    tokio::time::timeout(Duration::from_secs(20), async {
        let manager = Manager::new().await?;
        manager
            .adapters()
            .await?
            .into_iter()
            .next()
            .ok_or(BandError::NoAdapter)
    })
    .await
    .map_err(|_| BandError::BluetoothUnavailable)?
}

pub async fn scan(adapter: &Adapter, duration: Duration) -> Result<Vec<Peripheral>, BandError> {
    adapter.start_scan(ScanFilter::default()).await?;
    tokio::time::sleep(duration).await;
    adapter.stop_scan().await?;
    Ok(adapter.peripherals().await?)
}

pub async fn peripheral_name(p: &Peripheral) -> Option<String> {
    p.properties().await.ok().flatten().and_then(|x| x.local_name)
}

/// True when this peripheral is the band we are looking for.
async fn is_wanted(p: &Peripheral, peripheral_id: Option<&str>) -> bool {
    if let Some(wanted) = peripheral_id {
        return same_peripheral(&p.id().to_string(), wanted);
    }
    matches!(peripheral_name(p).await, Some(name) if name.starts_with(BAND_NAME_HINT))
}

/// Whether two ids name the same peripheral.
///
/// On macOS an id is a UUID. btleplug prints one in lower case, and a person,
/// or an older file, may write it in upper case, so two UUIDs are compared as
/// UUIDs. Compared as text, an upper-case id never matched, and the band was
/// never found. An id that is not a UUID, such as the address Windows uses,
/// is compared as text without regard to case.
pub fn same_peripheral(found: &str, wanted: &str) -> bool {
    let (found, wanted) = (found.trim(), wanted.trim());
    match (Uuid::parse_str(found), Uuid::parse_str(wanted)) {
        (Ok(found), Ok(wanted)) => found == wanted,
        _ => found.eq_ignore_ascii_case(wanted),
    }
}

/// Ask the platform for peripherals it is ALREADY connected to.
///
/// macOS auto-connects the band as a BLE HID device, and a connected peripheral
/// stops advertising, so scanning alone can never reach it while the system
/// holds it. This asks CoreBluetooth directly.
///
/// Only the service selector is used, never the identifier selector. The service
/// selector returns only currently-connected peripherals. The identifier
/// selector would also return a known peripheral that is out of range, and
/// connecting to that would block indefinitely.
///
/// A backend with no retrieval source returns an error; treat that as "nothing
/// found" rather than a failure, so other platforms still fall through to scanning.
/// Ask macOS for one known peripheral by its identifier.
///
/// This never scans, so it cannot trip the Bluetooth privacy abort. Prefer it
/// over the service lookup: macOS holds the band as an HID device, and a
/// connected-peripheral lookup by service only matches once the system has
/// discovered the Zepp service, which it does not always do.
#[cfg(target_os = "macos")]
async fn retrieve_by_id(adapter: &Adapter, peripheral_id: &str) -> Vec<Peripheral> {
    let Ok(uuid) = Uuid::parse_str(peripheral_id) else {
        tracing::debug!("peripheral_id is not a UUID, skipping the identifier lookup");
        return Vec::new();
    };
    let options = RetrievePeripheralsOptions {
        identifiers: Some(vec![PeripheralId::from(uuid)]),
        services: None,
    };
    match adapter.retrieve_peripherals(options).await {
        Ok(found) => found,
        Err(err) => {
            tracing::debug!("retrieve by identifier unavailable: {err}");
            Vec::new()
        }
    }
}

#[cfg(target_os = "macos")]
async fn retrieve_connected(adapter: &Adapter) -> Vec<Peripheral> {
    retrieve_with(adapter, vec![SERVICE_ZEPP]).await
}

/// The peripherals the system holds that carry any of these services. It never
/// scans, and a platform with no such lookup gives an empty list.
pub(crate) async fn retrieve_with(adapter: &Adapter, services: Vec<Uuid>) -> Vec<Peripheral> {
    let options = RetrievePeripheralsOptions {
        identifiers: None,
        services: Some(services),
    };
    match adapter.retrieve_peripherals(options).await {
        Ok(found) => found,
        Err(err) => {
            tracing::debug!("retrieve_peripherals unavailable: {err}");
            Vec::new()
        }
    }
}

/// Find the band by peripheral id when given, otherwise by advertised name.
///
/// Checks peripherals macOS already holds before scanning, because a band the
/// system has connected does not advertise and a scan can never see it.
/// How many times to ask macOS for a band it already holds before scanning.
#[cfg(target_os = "macos")]
const KNOWN_LOOKUP_TRIES: u32 = 120;
/// How long to wait between those tries.
#[cfg(target_os = "macos")]
const KNOWN_LOOKUP_PAUSE: Duration = Duration::from_millis(400);

#[cfg(target_os = "macos")]
pub async fn find_band(
    adapter: &Adapter,
    duration: Duration,
    peripheral_id: Option<&str>,
) -> Result<Peripheral, BandError> {
    // CoreBluetooth answers a lookup only once its adapter has powered on, and
    // that takes a moment after start. Ask repeatedly before falling back to a
    // scan: a scan aborts this process, because the binary carries no
    // NSBluetoothAlwaysUsageDescription.
    for _ in 0..KNOWN_LOOKUP_TRIES {
        if let Some(wanted) = peripheral_id {
            for candidate in retrieve_by_id(adapter, wanted).await {
                if is_wanted(&candidate, peripheral_id).await {
                    tracing::debug!("adopted a band the system holds, found by identifier");
                    return Ok(candidate);
                }
            }
        }

        for candidate in retrieve_connected(adapter).await {
            if is_wanted(&candidate, peripheral_id).await {
                tracing::debug!("adopted a band the system already holds");
                return Ok(candidate);
            }
        }

        tokio::time::sleep(KNOWN_LOOKUP_PAUSE).await;
    }

    // Never scan when the band is named. A scan aborts this process, and a
    // named band is always reachable through the lookup above once the band is
    // awake. Report instead, so the reason is readable.
    if peripheral_id.is_some() {
        return Err(BandError::BandNotFound);
    }

    for candidate in scan(adapter, duration).await? {
        if is_wanted(&candidate, peripheral_id).await {
            return Ok(candidate);
        }
    }

    Err(BandError::BandNotFound)
}

/// Find the band by peripheral id when given, otherwise by advertised name
/// (W7).
///
/// Windows names a peripheral by its Bluetooth address, which the config
/// already stores. It lets this program scan, and keeps no link to the band
/// for itself, as macOS does. So a scan finds the band, named or not, and it
/// ends as soon as the band is heard. There is nothing to ask the system for
/// first, and no long wait.
#[cfg(windows)]
pub async fn find_band(
    adapter: &Adapter,
    duration: Duration,
    peripheral_id: Option<&str>,
) -> Result<Peripheral, BandError> {
    adapter.start_scan(ScanFilter::default()).await?;
    let heard = heard_within(adapter, duration, peripheral_id).await;
    if let Err(err) = adapter.stop_scan().await {
        tracing::debug!("the scan did not stop: {err}");
    }
    heard?.ok_or(BandError::BandNotFound)
}

/// How often a running scan's findings are looked through.
#[cfg(windows)]
const SCAN_LOOK: Duration = Duration::from_millis(500);

/// The band, as soon as a running scan has heard it, or None when
/// `duration` passes first.
#[cfg(windows)]
async fn heard_within(
    adapter: &Adapter,
    duration: Duration,
    peripheral_id: Option<&str>,
) -> Result<Option<Peripheral>, BandError> {
    let until = tokio::time::Instant::now() + duration;
    loop {
        for candidate in adapter.peripherals().await? {
            if is_wanted(&candidate, peripheral_id).await {
                return Ok(Some(candidate));
            }
        }
        let now = tokio::time::Instant::now();
        if now >= until {
            return Ok(None);
        }
        tokio::time::sleep(SCAN_LOOK.min(until - now)).await;
    }
}

/// Connect, discover services, and locate the two chunked characteristics.
///
/// If anything fails after the link is open, the link is released. A Band 7
/// accepts one host at a time, so a half-open link starves the user's phone.
pub async fn connect_and_discover(p: &Peripheral) -> Result<Chunked, BandError> {
    if !p.is_connected().await? {
        p.connect().await?;
    }
    match discover_chunked(p).await {
        Ok(chunked) => Ok(chunked),
        Err(err) => {
            if let Err(cleanup) = p.disconnect().await {
                tracing::warn!("could not release the band after a failed connect: {cleanup}");
            }
            Err(err)
        }
    }
}

async fn discover_chunked(p: &Peripheral) -> Result<Chunked, BandError> {
    // btleplug fills Peripheral::mtu() during this call. Do not skip it.
    p.discover_services().await?;

    let all = p.characteristics();
    let write = all
        .iter()
        .find(|c| c.uuid == CHAR_WRITE)
        .cloned()
        .ok_or(BandError::MissingCharacteristic("0x16 chunked write"))?;
    let read = all
        .iter()
        .find(|c| c.uuid == CHAR_READ)
        .cloned()
        .ok_or(BandError::MissingCharacteristic("0x17 chunked read"))?;

    Ok(Chunked { write, read })
}

/// Locate the firmware service characteristics on an already-discovered band.
///
/// Call only after `connect_and_discover`, which performs service discovery.
pub fn firmware_characteristics(p: &Peripheral) -> Result<Firmware, BandError> {
    let all = p.characteristics();
    let control = all
        .iter()
        .find(|c| c.uuid == CHAR_FW_CONTROL)
        .cloned()
        .ok_or(BandError::MissingCharacteristic("0x1531 firmware control"))?;
    let data = all
        .iter()
        .find(|c| c.uuid == CHAR_FW_DATA)
        .cloned()
        .ok_or(BandError::MissingCharacteristic("0x1532 firmware data"))?;
    Ok(Firmware { control, data })
}

/// Prefer a write with response so chunk order is preserved.
pub fn write_type_for(c: &Characteristic) -> WriteType {
    if c.properties.contains(CharPropFlags::WRITE) {
        WriteType::WithResponse
    } else {
        WriteType::WithoutResponse
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T-003 found this. The file gave the id in upper case, btleplug gives
    /// it in lower case, the two never matched as text, and the window never
    /// found the band.
    #[test]
    fn an_upper_case_id_names_the_same_band() {
        let found = "01234567-89ab-4cde-8f01-23456789abcd";
        assert!(same_peripheral(found, "01234567-89AB-4CDE-8F01-23456789ABCD"));
        assert!(same_peripheral(found, " 01234567-89AB-4CDE-8F01-23456789ABCD\n"));
        assert!(same_peripheral(found, found));
        assert!(!same_peripheral(found, "01234567-89ab-4cde-8f01-23456789abce"));
    }

    /// Windows names a band by its address, which is not a UUID.
    #[test]
    fn an_id_that_is_not_a_uuid_is_compared_without_case() {
        assert!(same_peripheral("00:00:5e:00:53:01", "00:00:5E:00:53:01"));
        assert!(!same_peripheral("00:00:5e:00:53:01", "00:00:5E:00:53:02"));
        assert!(!same_peripheral("01234567-89ab-4cde-8f01-23456789abcd", "00:00:5E:00:53:01"));
    }
}
