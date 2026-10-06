//! Ask the band which apps are installed.
//!
//! Endpoint `0x00a0`, unencrypted: the same endpoint a custom app sends on.
//! Gadgetbridge calls this ZeppOsAppsService, and its first payload byte is a
//! type tag, so app management and an app's own bytes share the endpoint
//! without colliding.
//!
//! This asks and prints. It deletes nothing.

use bandbridge::{config, flow};
use std::time::Duration;

const ENDPOINT: u16 = 0x00a0;

/// The first byte: which kind of message this is.
const CMD_APPS: u8 = 0x02;
/// The second byte: to the band, or from it.
const TO_BAND: u8 = 0x01;
const FROM_BAND: u8 = 0x00;
/// The third byte: which apps command.
const APPS_LIST: u8 = 0x01;

/// Every reply carries its data from byte 16 on.
const DATA_AT: usize = 16;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = config::Config::load().unwrap_or_default();
    let key = config::AuthKey::load(config.mac())?;
    let (peripheral, mut session) =
        flow::open_session(key, config.uuid(), 20).await?;

    // Gadgetbridge sends 16 bytes: three command bytes, then zeros.
    let mut request = [0u8; 16];
    request[0] = CMD_APPS;
    request[1] = TO_BAND;
    request[2] = APPS_LIST;
    println!("asking: {}", hex::encode(request));
    session.send(ENDPOINT, &request, false).await?;

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        let Ok(message) = session.next_message(Duration::from_secs(2)).await else {
            continue;
        };
        if message.endpoint != ENDPOINT {
            continue;
        }
        let p = &message.payload;
        println!("reply: {}", hex::encode(p));
        if p.len() < 3 || p[0] != CMD_APPS || p[1] != FROM_BAND || p[2] != APPS_LIST {
            continue;
        }
        if p.len() <= DATA_AT {
            println!("the reply carries no list");
            break;
        }
        let text: Vec<u8> = p[DATA_AT..].iter().copied().take_while(|b| *b != 0).collect();
        let text = String::from_utf8_lossy(&text);
        println!("\ninstalled apps, as the band reports them:");
        for entry in text.split(';').filter(|e| !e.is_empty()) {
            match entry.split_once('-') {
                // The id is hex here, and the same number the manifest holds
                // in decimal.
                Some((id, version)) => match u32::from_str_radix(id, 16) {
                    Ok(number) => println!("  {id}  ({number})  version {version}"),
                    Err(_) => println!("  {entry}  (the id is not hex)"),
                },
                None => println!("  {entry}  (no version)"),
            }
        }
        break;
    }

    use btleplug::api::Peripheral as _;
    let _ = peripheral.disconnect().await;
    Ok(())
}
