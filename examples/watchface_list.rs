//! Ask the band which watch faces it holds, and which one it is wearing.
//!
//! Endpoint `0x0023`, encrypted. Gadgetbridge calls this
//! ZeppOsWatchfaceService.
//!
//! The point is to name the ids that the app list reports and nothing here
//! recognizes. The band gives every installed thing the same kind of id, so an
//! id in both lists is a watch face, not an app, and removing it would take
//! away a face rather than a program.
//!
//! This asks and prints. It changes nothing.

use bandbridge::{config, flow};
use std::time::Duration;

const ENDPOINT: u16 = 0x0023;

const CMD_LIST_GET: u8 = 0x05;
const CMD_LIST_RET: u8 = 0x06;
const CMD_CURRENT_GET: u8 = 0x09;
const CMD_CURRENT_RET: u8 = 0x0a;

fn codes_in(payload: &[u8]) -> Vec<u32> {
    // [command, status, count, then count codes of four bytes each]
    let count = *payload.get(2).unwrap_or(&0) as usize;
    (0..count)
        .filter_map(|i| {
            let at = 3 + i * 4;
            let four = payload.get(at..at + 4)?;
            Some(u32::from_le_bytes([four[0], four[1], four[2], four[3]]))
        })
        .collect()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = config::Config::load().unwrap_or_default();
    let key = config::AuthKey::load(config.mac())?;
    let (peripheral, mut session) =
        flow::open_session(key, config.uuid(), 20).await?;

    for (label, request, expect) in [
        ("installed", CMD_LIST_GET, CMD_LIST_RET),
        ("current", CMD_CURRENT_GET, CMD_CURRENT_RET),
    ] {
        session.send(ENDPOINT, &[request], true).await?;
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            let Ok(message) = session.next_message(Duration::from_secs(2)).await else {
                continue;
            };
            if message.endpoint != ENDPOINT || message.payload.first() != Some(&expect) {
                continue;
            }
            println!("\n{label}: {}", hex::encode(&message.payload));
            for code in codes_in(&message.payload) {
                println!("  {code:08X}  ({code})");
            }
            break;
        }
    }

    use btleplug::api::Peripheral as _;
    let _ = peripheral.disconnect().await;
    Ok(())
}
