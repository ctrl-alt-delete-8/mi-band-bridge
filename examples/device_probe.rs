//! What the band advertises about itself, and whether a MAC address is in it.
//!
//! macOS hides a Bluetooth address behind a CoreBluetooth UUID, so the address
//! has to come from the band's own advertisement, if it is there at all.

use bandbridge::{ble, config};
use btleplug::api::Peripheral as _;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = config::Config::load().unwrap_or_default();
    let adapter = ble::adapter().await?;

    // Everything the adapter hears while a scan runs, before any connection.
    // An advertisement carries manufacturer data; a connected peripheral may
    // not report it any more.
    println!("--- what the scan hears ---");
    for found in ble::scan(&adapter, Duration::from_secs(10)).await? {
        let Some(props) = found.properties().await? else {
            continue;
        };
        let name = props.local_name.clone().unwrap_or_default();
        if !name.contains("Band") && !props.manufacturer_data.is_empty() {
            continue;
        }
        println!("{} {} address={}", found.id(), name, props.address);
        for (company, data) in &props.manufacturer_data {
            println!("    manufacturer {company:#06x}: {}", hex::encode(data));
        }
        for (uuid, data) in &props.service_data {
            println!("    service data {uuid}: {}", hex::encode(data));
        }
    }

    println!("--- the band itself ---");
    let peripheral = ble::find_band(
        &adapter,
        Duration::from_secs(20),
        config.uuid(),
    )
    .await?;

    println!("peripheral id: {}", peripheral.id());
    let Some(props) = peripheral.properties().await? else {
        println!("no properties");
        return Ok(());
    };
    println!("address:       {} ({:?})", props.address, props.address_type);
    println!("local name:    {:?}", props.local_name);
    println!("rssi:          {:?}", props.rssi);
    for (company, data) in &props.manufacturer_data {
        println!("manufacturer {company:#06x}: {}", hex::encode(data));
    }
    for (uuid, data) in &props.service_data {
        println!("service data {uuid}: {}", hex::encode(data));
    }
    for uuid in &props.services {
        println!("advertised service: {uuid}");
    }

    // The standard Device Information service. System ID, 0x2A23, is a 64 bit
    // value that many devices build from their MAC address.
    println!("--- device information service ---");
    ble::connect_and_discover(&peripheral).await?;
    let wanted = [
        ("2a23 system id", "00002a23-0000-1000-8000-00805f9b34fb"),
        ("2a25 serial", "00002a25-0000-1000-8000-00805f9b34fb"),
        ("2a27 hardware", "00002a27-0000-1000-8000-00805f9b34fb"),
        ("2a28 software", "00002a28-0000-1000-8000-00805f9b34fb"),
        ("2a50 pnp id", "00002a50-0000-1000-8000-00805f9b34fb"),
        ("2a19 battery", "00002a19-0000-1000-8000-00805f9b34fb"),
    ];
    for characteristic in peripheral.characteristics() {
        let uuid = characteristic.uuid.to_string();
        let Some((name, _)) = wanted.iter().find(|(_, id)| *id == uuid) else {
            continue;
        };
        match peripheral.read(&characteristic).await {
            Ok(value) => println!(
                "{name}: {} | {}",
                hex::encode(&value),
                String::from_utf8_lossy(&value).trim_end_matches('\0')
            ),
            Err(err) => println!("{name}: cannot read: {err}"),
        }
    }
    peripheral.disconnect().await?;
    Ok(())
}
