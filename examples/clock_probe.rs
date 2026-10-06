//! Write the clock, then read it back, to see what the band kept.
use bandbridge::{band_time, config, flow};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = config::Config::load().unwrap_or_default();
    let key = config::AuthKey::load(config.mac())?;
    let (peripheral, session) =
        flow::open_session(key, config.uuid(), 20).await?;

    let show = |label: &str, bytes: &[u8]| {
        println!("{label:8} {}", bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "));
        if bytes.len() >= 7 {
            println!("         {:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                u16::from_le_bytes([bytes[0], bytes[1]]), bytes[2], bytes[3],
                bytes[4], bytes[5], bytes[6]);
        }
    };

    match session.read_characteristic(band_time::CHAR_CURRENT_TIME).await {
        Ok(before) => show("before", &before),
        Err(err) => println!("before   read failed: {err}"),
    }

    let command = band_time::set_now();
    show("writing", &command);
    match session.write_characteristic(band_time::CHAR_CURRENT_TIME, &command).await {
        Ok(()) => println!("write    accepted"),
        Err(err) => println!("write    failed: {err}"),
    }

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    match session.read_characteristic(band_time::CHAR_CURRENT_TIME).await {
        Ok(after) => show("after", &after),
        Err(err) => println!("after    read failed: {err}"),
    }

    use btleplug::api::Peripheral as _;
    let _ = peripheral.disconnect().await;
    Ok(())
}
