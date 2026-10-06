//! Ask the band which chunked services it supports.
use bandbridge::{config, flow};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = config::Config::load().unwrap_or_default();
    let key = config::AuthKey::load(config.mac())?;
    let (peripheral, mut session) =
        flow::open_session(key, config.uuid(), 20).await?;

    // ZeppOsServicesService is built unencrypted, so ask that way first.
    for encrypt in [false, true] {
        session.send(0x0000, &[0x03], encrypt).await?;
        let deadline = std::time::Instant::now() + Duration::from_secs(6);
        while std::time::Instant::now() < deadline {
            let Ok(message) = session.next_message(Duration::from_secs(2)).await else {
                continue;
            };
            if message.endpoint != 0x0000 || message.payload.first() != Some(&0x04) {
                continue;
            }
            let p = &message.payload;
            let count = u16::from_le_bytes([p[1], p[2]]) as usize;
            println!("encrypted={encrypt}  services={count}");
            for i in 0..count {
                let at = 3 + i * 3;
                if at + 2 >= p.len() {
                    break;
                }
                let endpoint = u16::from_le_bytes([p[at], p[at + 1]]);
                let enc = p[at + 2];
                let mark = if endpoint == 0x0047 { "  <-- time service" } else { "" };
                println!("  {endpoint:#06x} encrypted={enc}{mark}");
            }
            use btleplug::api::Peripheral as _;
            let _ = peripheral.disconnect().await;
            return Ok(());
        }
        println!("no reply with encrypted={encrypt}");
    }

    use btleplug::api::Peripheral as _;
    let _ = peripheral.disconnect().await;
    Ok(())
}
