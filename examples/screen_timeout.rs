//! Read the band's screen-on time, and try to set it.
//!
//! The band's own menu offers 5 to 15 seconds, and the config service declares
//! the same eleven values. This asks whether that cap is advisory or enforced.
//!
//! ```sh
//! cargo run --example screen_timeout            # read
//! cargo run --example screen_timeout -- 30      # try to set
//! ```

use bandbridge::{config, flow};
use bandbridge::session::Session;
use std::time::Duration;

const ENDPOINT: u16 = 0x000a;
const CMD_REQUEST: u8 = 0x03;
const CMD_RESPONSE: u8 = 0x04;
const CMD_SET: u8 = 0x05;
const CMD_ACK: u8 = 0x06;

const GROUP_DISPLAY: u8 = 0x01;
const GROUP_VERSION: u8 = 0x02;
const ARG_SCREEN_TIMEOUT: u8 = 0x03;
const TYPE_BYTE: u8 = 0x10;

/// Ask for the screen timeout, and return the value and the allowed set.
async fn read_timeout(session: &mut Session) -> Result<(u8, Vec<u8>), Box<dyn std::error::Error>> {
    // Constraints on, and one arg code, so the reply carries the allowed set.
    let request = [CMD_REQUEST, 0x01, GROUP_DISPLAY, 0x01, ARG_SCREEN_TIMEOUT];
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
        if p.first() != Some(&CMD_RESPONSE) || p.len() < 9 || p[1] != 1 {
            continue;
        }
        // 04 01 group version constraints count | arg type value count values...
        if p[6] != ARG_SCREEN_TIMEOUT || p[7] != TYPE_BYTE {
            continue;
        }
        let value = p[8];
        let allowed = p
            .get(9)
            .map(|count| {
                let n = *count as usize;
                p.get(10..10 + n).unwrap_or_default().to_vec()
            })
            .unwrap_or_default();
        return Ok((value, allowed));
    }
    Err("the band did not report its screen timeout".into())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wanted: Option<u8> = std::env::args().nth(1).and_then(|a| a.parse().ok());

    let config = config::Config::load().unwrap_or_default();
    let key = config::AuthKey::load(config.mac())?;
    let (peripheral, mut session) =
        flow::open_session(key, config.uuid(), 20).await?;

    let outcome = async {
        let (before, allowed) = read_timeout(&mut session).await?;
        println!("screen timeout is {before} seconds");
        println!(
            "the band says it allows: {}",
            allowed
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );

        let Some(wanted) = wanted else {
            return Ok::<(), Box<dyn std::error::Error>>(());
        };

        // CMD_SET, group, version, a byte Gadgetbridge sends as zero, then the
        // count of settings, then each as code, type, value.
        let set = [
            CMD_SET,
            GROUP_DISPLAY,
            GROUP_VERSION,
            0x00,
            0x01,
            ARG_SCREEN_TIMEOUT,
            TYPE_BYTE,
            wanted,
        ];
        println!("\nsending {}", hex::encode(set));
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

        // Read back rather than trust the answer.
        let (after, _) = read_timeout(&mut session).await?;
        println!("screen timeout is now {after} seconds");
        if after != wanted {
            println!("the band kept {after}, so the cap is enforced, not advisory");
        }
        Ok(())
    }
    .await;

    use btleplug::api::Peripheral as _;
    let _ = peripheral.disconnect().await;
    outcome
}
