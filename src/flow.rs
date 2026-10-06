//! Whole jobs, from opening a link to releasing it.
//!
//! A window button and a command both call one of these. Neither front end
//! knows how a job is done, so the two cannot drift apart.

use btleplug::api::Peripheral as _;

use crate::ble;
use crate::config::AuthKey;
use crate::error::BandError;
use crate::install;
use crate::screens;
use crate::session::Session;
use crate::supervisor;
use std::time::Duration;

/// Open a link to the band and authenticate.
///
/// The caller owns the session, and must release the peripheral when done.
pub async fn open_session(
    auth_key: AuthKey,
    peripheral_id: Option<&str>,
    seconds: u64,
) -> Result<(btleplug::platform::Peripheral, Session), BandError> {
    let adapter = ble::adapter().await?;
    let peripheral =
        ble::find_band(&adapter, Duration::from_secs(seconds), peripheral_id).await?;
    let chunked = ble::connect_and_discover(&peripheral).await?;
    let mut session = Session::open(peripheral.clone(), chunked, false, None, false).await?;
    supervisor::authenticate(&mut session, auth_key).await?;
    Ok((peripheral, session))
}

/// Install an app, then put the app menu back in order.
///
/// The band decides where a new app lands, so the menu is rebuilt afterwards.
/// That needs the chunked transport, which the install tears down, so it takes
/// a second link. A failure there leaves the app installed, and says so.
pub async fn install_app(
    auth_key: AuthKey,
    peripheral_id: Option<&str>,
    seconds: u64,
    file: &str,
) -> Result<Vec<String>, BandError> {
    let bytes = std::fs::read(file)
        .map_err(|err| BandError::Install(format!("cannot read {file}: {err}")))?;
    if bytes.len() as u64 > install::MAX_APP_BYTES {
        return Err(BandError::FileTooLarge(bytes.len() as u64));
    }
    // The band will report this app as an id and nothing else, so read its
    // name now, while the package and its manifest are in front of us.
    let newest = match crate::app_names::from_package(std::path::Path::new(file)) {
        Some((id, name)) => {
            if let Err(err) = crate::app_names::remember(&id, &name) {
                tracing::warn!("could not remember the name of {id}: {err}");
            }
            Some(id)
        }
        None => None,
    };

    let mut lines = vec![format!("{file}: {} bytes", bytes.len())];

    let adapter = ble::adapter().await?;
    let peripheral =
        ble::find_band(&adapter, Duration::from_secs(seconds), peripheral_id).await?;
    let chunked = ble::connect_and_discover(&peripheral).await?;

    // Keep a handle on the chunked read characteristic: it must be unsubscribed
    // before the firmware transfer, or the band refuses the first command.
    let read_char = chunked.read.clone();
    let outcome = async {
        let mut session =
            Session::open(peripheral.clone(), chunked, false, None, false).await?;
        supervisor::authenticate(&mut session, auth_key.clone()).await?;
        let fw = ble::firmware_characteristics(&peripheral)?;
        // The terminal sees the upload go from the transfer's own lines.
        install::run(&peripheral, &fw, &read_char, &bytes, |_, _| {}).await
    }
    .await;

    if let Err(err) = peripheral.disconnect().await {
        tracing::warn!("could not release the band: {err}");
    }
    outcome?;
    lines.push("Installed.".to_string());

    lines.push("Tidying the app menu...".to_string());
    match tidy_menu(auth_key, peripheral_id, seconds, newest.as_deref()).await {
        Ok(more) => lines.extend(more),
        Err(err) => lines.push(format!(
            "The app is installed. The menu could not be tidied: {err}"
        )),
    }
    Ok(lines)
}

/// Rebuild the app menu on a link of its own.
pub async fn tidy_menu(
    auth_key: AuthKey,
    peripheral_id: Option<&str>,
    seconds: u64,
    newest: Option<&str>,
) -> Result<Vec<String>, BandError> {
    let (peripheral, mut session) = open_session(auth_key, peripheral_id, seconds).await?;
    let result = screens::tidy_menu(&mut session, newest).await;
    if let Err(err) = peripheral.disconnect().await {
        tracing::warn!("could not release the band: {err}");
    }
    result
}
