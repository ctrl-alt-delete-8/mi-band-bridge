//! Read the band's display settings from the config service.
//!
//! Endpoint `0x000a`, encrypted. Gadgetbridge calls this ZeppOsConfigService.
//! This asks and prints. It writes nothing.

use bandbridge::{config, flow};
use std::time::Duration;

/// The config service, as the band listed it.
const ENDPOINT: u16 = 0x000a;

const CMD_REQUEST: u8 = 0x03;
const CMD_RESPONSE: u8 = 0x04;

/// Group 0x01 is Display, at version 0x02.
const GROUP_DISPLAY: u8 = 0x01;

/// Names for the display group, from Gadgetbridge's ConfigArg.
fn display_arg(code: u8) -> &'static str {
    match code {
        0x01 => "screen_auto_brightness",
        0x02 => "screen_brightness",
        0x03 => "screen_timeout",
        0x04 => "always_on_display_mode",
        0x05 => "always_on_display_start",
        0x06 => "always_on_display_end",
        0x08 => "lift_wrist_mode",
        0x09 => "lift_wrist_start",
        0x0a => "lift_wrist_end",
        0x0b => "lift_wrist_sensitivity",
        0x0c => "screen_on_on_notifications",
        0x0d => "workout_keep_screen_on",
        0x0e => "always_on_display_follow_watchface",
        0x0f => "always_on_display_style",
        _ => "unknown",
    }
}

fn type_name(code: u8) -> &'static str {
    match code {
        0x01 => "short",
        0x02 => "short_list",
        0x03 => "int",
        0x0b => "bool",
        0x10 => "byte",
        0x11 => "byte_list",
        0x20 => "string",
        0x21 => "string_list",
        0x30 => "hh:mm",
        0x40 => "timestamp",
        0x50 => "int_unbound",
        _ => "?",
    }
}

/// Walk a response that carries no constraints, where a value is just itself.
fn print_plain(items: &[u8]) {
    let mut at = 0;
    while at + 1 < items.len() {
        let arg = items[at];
        let kind = items[at + 1];
        at += 2;
        let width = match kind {
            0x0b | 0x10 => 1,
            0x01 | 0x30 => 2,
            0x03 => 4,
            _ => {
                println!(
                    "  {:#04x} {:<34} {:<10} stopping: this type has no fixed width",
                    arg,
                    display_arg(arg),
                    type_name(kind)
                );
                println!("  rest: {}", hex::encode(&items[at..]));
                return;
            }
        };
        if at + width > items.len() {
            println!("  {:#04x} {} ran off the end", arg, display_arg(arg));
            return;
        }
        let value = &items[at..at + width];
        at += width;
        let shown = match width {
            1 => format!("{}", value[0]),
            2 if kind == 0x30 => format!("{:02}:{:02}", value[0], value[1]),
            2 => format!("{}", u16::from_le_bytes([value[0], value[1]])),
            _ => hex::encode(value),
        };
        println!(
            "  {:#04x} {:<34} {:<10} {}",
            arg,
            display_arg(arg),
            type_name(kind),
            shown
        );
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = config::Config::load().unwrap_or_default();
    let key = config::AuthKey::load(config.mac())?;
    let (peripheral, mut session) =
        flow::open_session(key, config.uuid(), 20).await?;

    // Every group the band might answer. Gadgetbridge names all but 0x02,
    // which it marks as a TODO, so that one is worth a look.
    let groups: Vec<u8> = match std::env::args().nth(1) {
        Some(arg) => vec![arg.trim_start_matches("0x").parse().unwrap_or(GROUP_DISPLAY)],
        None => (0x00..=0x0b).collect(),
    };

    for group in groups {
        // CMD_REQUEST, constraints on, the group, then zero arg codes, which
        // asks for every setting the group holds.
        let request = [CMD_REQUEST, 0x01, group, 0x00];
        println!("\n=== group {group:#04x}: {} ===", hex::encode(request));
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
            println!("reply: {}", hex::encode(p));
            if p.first() != Some(&CMD_RESPONSE) || p.len() < 6 {
                continue;
            }
            if p[1] != 1 {
                println!("the band refused the request, status {:#04x}", p[1]);
                break;
            }
            println!(
                "group={:#04x} version={:#04x} constraints={} settings={}",
                p[2], p[3], p[4], p[5]
            );
            if p[4] == 0 {
                print_plain(&p[6..]);
            } else {
                println!("  raw: {}", hex::encode(&p[6..]));
            }
            break;
        }
    }

    use btleplug::api::Peripheral as _;
    let _ = peripheral.disconnect().await;
    Ok(())
}
