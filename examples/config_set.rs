//! Set one boolean on the band's config service, and read it back.
//!
//! For trying the settings Gadgetbridge does not name. Every write is read
//! back, so a refusal never looks like a success.
//!
//! ```sh
//! cargo run --example config_set -- 0x0a 0x09      # read one setting
//! cargo run --example config_set -- 0x0a 0x09 0    # set it to false
//! ```

use bandbridge::session::Session;
use bandbridge::{config, flow};
use std::time::Duration;

const ENDPOINT: u16 = 0x000a;
const CMD_REQUEST: u8 = 0x03;
const CMD_RESPONSE: u8 = 0x04;
const CMD_SET: u8 = 0x05;
const CMD_ACK: u8 = 0x06;
const TYPE_BOOL: u8 = 0x0b;

fn parse_hex(text: &str) -> Option<u8> {
    match text.strip_prefix("0x") {
        Some(rest) => u8::from_str_radix(rest, 16).ok(),
        None => text.parse().ok(),
    }
}

/// Ask for one setting. Returns the group's version and the value.
///
/// The version is read rather than assumed: the band refuses a write that
/// names a version it does not keep, and the versions differ per group.
async fn read_one(
    session: &mut Session,
    group: u8,
    arg: u8,
) -> Result<(u8, u8), Box<dyn std::error::Error>> {
    // Constraints off keeps the reply simple: a bool is one byte.
    let request = [CMD_REQUEST, 0x00, group, 0x01, arg];
    session.send(ENDPOINT, &request, true).await?;

    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    while std::time::Instant::now() < deadline {
        let Ok(message) = session.next_message(Duration::from_secs(2)).await else {
            continue;
        };
        if message.endpoint != ENDPOINT {
            continue;
        }
        let p = &message.payload;
        if p.first() != Some(&CMD_RESPONSE) || p.len() < 9 || p[1] != 1 || p[2] != group {
            continue;
        }
        if p[6] != arg || p[7] != TYPE_BOOL {
            return Err(format!(
                "the band answered arg {:#04x} type {:#04x}, not a bool for {arg:#04x}",
                p[6], p[7]
            )
            .into());
        }
        return Ok((p[3], p[8]));
    }
    Err(format!("the band did not report {group:#04x}/{arg:#04x}").into())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let group = args.first().and_then(|a| parse_hex(a)).ok_or("give a group")?;
    let arg = args.get(1).and_then(|a| parse_hex(a)).ok_or("give an arg")?;
    let wanted = args.get(2).and_then(|a| parse_hex(a));

    let config = config::Config::load().unwrap_or_default();
    let key = config::AuthKey::load(config.mac())?;
    let (peripheral, mut session) =
        flow::open_session(key, config.uuid(), 20).await?;

    let outcome = async {
        let (version, before) = read_one(&mut session, group, arg).await?;
        println!("group {group:#04x} version {version:#04x}: {arg:#04x} = {before}");

        let Some(wanted) = wanted else {
            return Ok::<(), Box<dyn std::error::Error>>(());
        };

        let set = [CMD_SET, group, version, 0x00, 0x01, arg, TYPE_BOOL, wanted];
        println!("sending {}", hex::encode(set));
        session.send(ENDPOINT, &set, true).await?;

        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            let Ok(message) = session.next_message(Duration::from_secs(2)).await else {
                continue;
            };
            if message.endpoint == ENDPOINT && message.payload.first() == Some(&CMD_ACK) {
                println!("the band answered status {:#04x}", message.payload[1]);
                break;
            }
        }

        let (_, after) = read_one(&mut session, group, arg).await?;
        println!("{arg:#04x} is now {after}");
        if after == before {
            println!("unchanged. The band kept {before}");
        } else {
            println!("changed from {before} to {after}");
            println!("to put it back: cargo run --example config_set -- {group:#04x} {arg:#04x} {before}");
        }
        Ok(())
    }
    .await;

    use btleplug::api::Peripheral as _;
    let _ = peripheral.disconnect().await;
    outcome
}
