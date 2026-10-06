//! Finding the band that takes a key.
//!
//! A person pairs a band with one value, the auth key the account login gives
//! (decisions D1 and D2). Nothing else names the band. So BandBridge tries the
//! key on each band in reach, strongest signal first, and the first band that
//! accepts it is the one. A band that refuses it is let go at once. This
//! computer's id for the band is found on the way, and the band's MAC is read
//! from the band.
//!
//! [`first_to_accept`] holds the order and the outcome, and no Bluetooth, so
//! stand-ins test it. [`bands_nearby`] is what it looks with on a real band.

use crate::ble::{self, BAND_NAME_HINT, SERVICE_ZEPP};
use crate::error::BandError;
use btleplug::api::{Central as _, Peripheral as _};
use btleplug::platform::{Adapter, Peripheral};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::future::Future;
use std::time::{Duration, Instant};
use uuid::{Uuid, uuid};

/// How long a pair looks for bands.
///
/// The band advertises only now and then: a scan of six seconds finds it about
/// half the time, and one of twenty to thirty seconds finds it every time.
pub const SEARCH: Duration = Duration::from_secs(25);

/// How long to wait between two looks.
pub const PAUSE: Duration = Duration::from_secs(1);

/// The longest one band may take to accept or refuse the key. A band that
/// takes longer is let go, and the search goes on.
pub const TRY_LIMIT: Duration = Duration::from_secs(30);

/// Human Interface Device. macOS holds a band it has paired through this
/// service, and a band it holds does not advertise.
const SERVICE_HID: Uuid = uuid!("00001812-0000-1000-8000-00805f9b34fb");

/// Device Information, which the band's MAC is read from.
const SERVICE_DEVICE_INFORMATION: Uuid = uuid!("0000180a-0000-1000-8000-00805f9b34fb");

/// What the pairing lines call a band that gives no name. Its id would read
/// as a UUID, and the window shows none (D2).
const NO_NAME: &str = "a band with no name";

/// One band in reach.
#[derive(Debug, Clone)]
pub struct Seen<B> {
    /// This computer's id for the band. A search asks each id once.
    pub id: String,
    /// What the band calls itself, when it says.
    pub name: Option<String>,
    /// The system holds the band already. A band the system holds does not
    /// advertise, so it has no signal to rank it by. It is connected to this
    /// Mac, which no stranger's band is, so it is asked first.
    pub held: bool,
    /// The signal of the band's last advertisement, in dBm.
    pub rssi: Option<i16>,
    /// The band itself.
    pub band: B,
}

impl<B> Seen<B> {
    /// The band's name, or words that say it gave none. Never its id.
    pub fn name(&self) -> String {
        self.name.clone().unwrap_or_else(|| NO_NAME.to_string())
    }

    /// What a pair says as it tries this band: its name, and what happens
    /// next. Why the band comes where it does in the order, held by the
    /// system or by its signal, is for developers, and goes to the log.
    pub fn trying(&self) -> String {
        format!("found {}, connecting and checking the key", self.name())
    }
}

/// What a band did with the key.
pub enum Asked<L> {
    /// It accepted the key. Its link stays open.
    Accepted(L),
    /// It answered, and refused the key. Its link is let go.
    Refused,
    /// It could not be asked: the link or the handshake failed before the band
    /// could judge the key. Its link is let go. The reason never holds the key.
    Failed(String),
}

/// The order to ask bands in: the ones the system holds first, then the rest
/// by signal, strongest first, and a band with no signal last. Bands that tie
/// keep the order they were found in.
fn strongest_first<B>(a: &Seen<B>, b: &Seen<B>) -> Ordering {
    b.held
        .cmp(&a.held)
        .then_with(|| b.rssi.unwrap_or(i16::MIN).cmp(&a.rssi.unwrap_or(i16::MIN)))
}

/// Ask each band as it comes into reach for the key, strongest signal first,
/// until one accepts it.
///
/// Each round looks, then asks every band it has not asked yet, in the order
/// of [`strongest_first`]. A band first seen in a later round waits for that
/// round, even when its signal is stronger than one asked already. Rounds go
/// on until `until`, the bands found by then are all asked, and no band is
/// asked twice. The first band to accept ends the search, and the bands after
/// it are never asked.
///
/// With none accepting, the error says what the search found: no band at
/// all, bands that refused the key, or bands that could not be asked.
pub async fn first_to_accept<B, L, Look, Ask>(
    until: Instant,
    pause: Duration,
    mut look: impl FnMut() -> Look,
    mut ask: impl FnMut(Seen<B>) -> Ask,
) -> Result<L, BandError>
where
    Look: Future<Output = Vec<Seen<B>>>,
    Ask: Future<Output = Asked<L>>,
{
    let mut asked: HashSet<String> = HashSet::new();
    let mut refused = 0;
    let mut failed: Vec<String> = Vec::new();
    loop {
        let mut round = look().await;
        round.sort_by(strongest_first);
        for seen in round {
            // A band held and heard at once is asked once, as the held one,
            // which the order puts first.
            if !asked.insert(seen.id.clone()) {
                continue;
            }
            match ask(seen).await {
                Asked::Accepted(link) => return Ok(link),
                Asked::Refused => refused += 1,
                Asked::Failed(why) => failed.push(why),
            }
        }
        let now = Instant::now();
        if now >= until {
            break;
        }
        tokio::time::sleep(pause.min(until - now)).await;
    }
    Err(nobody_took_it(asked.len(), refused, &failed))
}

/// Why a search that found no band to take the key ended as it did.
///
/// `found` counts the bands asked, `refused` the ones that said no, and
/// `failed` holds why each of the rest could not be asked.
fn nobody_took_it(found: usize, refused: usize, failed: &[String]) -> BandError {
    if found == 0 {
        return BandError::NoBandNearby;
    }
    let unasked = match failed {
        [] => String::new(),
        [only] => format!(", and 1 could not be asked ({only})"),
        [.., last] => format!(", and {} could not be asked (the last: {last})", failed.len()),
    };
    match (refused, failed) {
        (0, [only]) => BandError::BandUnreachable(format!(
            "a band is nearby, but Mi Band Bridge could not ask it for the key: {only}. Try again."
        )),
        (0, [.., last]) => BandError::BandUnreachable(format!(
            "{} bands are nearby, but Mi Band Bridge could not ask any of them for the key. \
             The last: {last}. Try again.",
            failed.len()
        )),
        (1, _) => BandError::KeyRefused(format!(
            "1 band refused it{unasked}. Check that this is the key the account login gave \
             for this band."
        )),
        (refused, _) => BandError::KeyRefused(format!(
            "{refused} bands refused it{unasked}. Check that this is the key the account \
             login gave for this band."
        )),
    }
}

/// Whether a name is one a band gives itself.
fn named_like_a_band(name: &str) -> bool {
    name.starts_with(BAND_NAME_HINT)
}

/// Every band in reach, for one round of a search.
///
/// The system may hold a band already. macOS keeps a band it has paired
/// connected, through the HID service, and a connected band stops
/// advertising, so no scan can hear it. So the system is asked first for the
/// bands it holds: those with the Zepp service, and, because the system does
/// not always know that service is there, those with HID or Device
/// Information that carry a band's name. Then come the bands a running scan
/// has heard advertise the Zepp service, or a band's name.
///
/// The scan is the caller's to start and stop. Asking the system needs none.
pub async fn bands_nearby(adapter: &Adapter) -> Vec<Seen<Peripheral>> {
    let mut seen = Vec::new();
    for peripheral in ble::retrieve_with(adapter, vec![SERVICE_ZEPP]).await {
        let name = ble::peripheral_name(&peripheral).await;
        seen.push(held(peripheral, name));
    }
    let others = vec![SERVICE_HID, SERVICE_DEVICE_INFORMATION];
    for peripheral in ble::retrieve_with(adapter, others).await {
        let name = ble::peripheral_name(&peripheral).await;
        if name.as_deref().is_some_and(named_like_a_band) {
            seen.push(held(peripheral, name));
        }
    }
    for peripheral in adapter.peripherals().await.unwrap_or_default() {
        let Ok(Some(heard)) = peripheral.properties().await else {
            continue;
        };
        let zepp = heard.services.contains(&SERVICE_ZEPP)
            || heard.service_data.contains_key(&SERVICE_ZEPP);
        let named = heard.local_name.as_deref().is_some_and(named_like_a_band);
        if zepp || named {
            seen.push(Seen {
                id: peripheral.id().to_string(),
                name: heard.local_name,
                held: false,
                rssi: heard.rssi,
                band: peripheral,
            });
        }
    }
    seen
}

fn held(peripheral: Peripheral, name: Option<String>) -> Seen<Peripheral> {
    Seen {
        id: peripheral.id().to_string(),
        name,
        held: true,
        rssi: None,
        band: peripheral,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A band for the tests: an id, whether the system holds it, its signal,
    /// and what it does with the key.
    fn band(id: &str, held: bool, rssi: Option<i16>, does: Does) -> Seen<Does> {
        Seen {
            id: id.to_string(),
            name: Some(format!("band {id}")),
            held,
            rssi,
            band: does,
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Does {
        Accept,
        Refuse,
        Fail,
    }

    /// Run a search over these rounds, the last one repeating, and give back
    /// its outcome and the ids it asked, in order. The search ends after one
    /// round unless `rounds` asks for time.
    async fn search(rounds: Vec<Vec<Seen<Does>>>, time: Duration) -> (Result<String, BandError>, Vec<String>) {
        let rounds = Mutex::new(rounds.into_iter());
        let last = Mutex::new(Vec::new());
        let asked = Mutex::new(Vec::new());
        let result = first_to_accept(
            Instant::now() + time,
            Duration::from_millis(1),
            || {
                let next = rounds.lock().unwrap().next();
                let round = match next {
                    Some(round) => {
                        *last.lock().unwrap() = round.clone();
                        round
                    }
                    None => last.lock().unwrap().clone(),
                };
                async move { round }
            },
            |seen: Seen<Does>| {
                asked.lock().unwrap().push(seen.id.clone());
                async move {
                    match seen.band {
                        Does::Accept => Asked::Accepted(seen.id),
                        Does::Refuse => Asked::Refused,
                        Does::Fail => Asked::Failed("timed out waiting for the band".to_string()),
                    }
                }
            },
        )
        .await;
        (result, asked.into_inner().unwrap())
    }

    /// The system's own bands first, then the strongest signal, and a band
    /// with no signal last.
    #[tokio::test]
    async fn a_held_band_goes_first_then_the_strongest_signal() {
        let round = vec![
            band("weak", false, Some(-80), Does::Refuse),
            band("quiet", false, None, Does::Refuse),
            band("strong", false, Some(-50), Does::Refuse),
            band("held", true, None, Does::Refuse),
        ];
        let (result, asked) = search(vec![round], Duration::ZERO).await;
        assert_eq!(asked, ["held", "strong", "weak", "quiet"]);
        let err = result.unwrap_err();
        assert!(matches!(err, BandError::KeyRefused(_)), "{err}");
        assert!(err.to_string().contains("4 bands refused it"), "{err}");
    }

    /// The first band to accept is the one, and nobody after it is asked.
    #[tokio::test]
    async fn the_first_band_to_accept_ends_the_search() {
        let round = vec![
            band("second", false, Some(-60), Does::Accept),
            band("first", false, Some(-40), Does::Refuse),
            band("third", false, Some(-70), Does::Accept),
        ];
        let (result, asked) = search(vec![round], Duration::from_secs(5)).await;
        assert_eq!(result.unwrap(), "second");
        assert_eq!(asked, ["first", "second"]);
    }

    /// A band heard later waits for its round, and no band is asked twice,
    /// however many rounds it is seen in.
    #[tokio::test]
    async fn a_band_heard_later_is_asked_in_its_round_and_once() {
        let rounds = vec![
            vec![band("early", false, Some(-80), Does::Refuse)],
            vec![],
            vec![
                band("early", false, Some(-80), Does::Refuse),
                band("late", false, Some(-40), Does::Refuse),
            ],
        ];
        let (result, asked) = search(rounds, Duration::from_millis(40)).await;
        assert_eq!(asked, ["early", "late"]);
        assert!(result.unwrap_err().to_string().contains("2 bands refused it"));
    }

    /// A band the system holds and a scan hears too is one band, asked once.
    #[tokio::test]
    async fn a_band_held_and_heard_is_asked_once() {
        let round = vec![
            band("band", false, Some(-55), Does::Refuse),
            band("band", true, None, Does::Refuse),
        ];
        let (_, asked) = search(vec![round], Duration::ZERO).await;
        assert_eq!(asked, ["band"]);
    }

    #[tokio::test]
    async fn no_band_in_reach_is_no_band_nearby() {
        let (result, asked) = search(vec![vec![]], Duration::from_millis(10)).await;
        assert!(asked.is_empty());
        let err = result.unwrap_err();
        assert!(matches!(err, BandError::NoBandNearby), "{err}");
        assert!(err.to_string().starts_with("no band is nearby"), "{err}");
    }

    /// A band that could not be asked is not a wrong key. Saying so would send
    /// the person to check a key that may be right.
    #[tokio::test]
    async fn a_band_that_could_not_be_asked_is_not_a_wrong_key() {
        let (result, _) = search(vec![vec![band("far", false, Some(-90), Does::Fail)]], Duration::ZERO).await;
        let err = result.unwrap_err();
        assert!(matches!(err, BandError::BandUnreachable(_)), "{err}");
        assert_eq!(
            err.to_string(),
            "a band is nearby, but Mi Band Bridge could not ask it for the key: timed out \
             waiting for the band. Try again."
        );
    }

    /// Refusals and failures together say both, so the person knows the key
    /// was not tried on every band.
    #[tokio::test]
    async fn a_refusal_and_a_failure_say_both() {
        let round = vec![
            band("near", false, Some(-50), Does::Refuse),
            band("far", false, Some(-90), Does::Fail),
        ];
        let (result, _) = search(vec![round], Duration::ZERO).await;
        let err = result.unwrap_err().to_string();
        assert_eq!(
            err,
            "no band nearby accepts this key: 1 band refused it, and 1 could not be asked \
             (timed out waiting for the band). Check that this is the key the account login \
             gave for this band."
        );
    }

    /// The line a pair says as it tries a band names the band and what
    /// happens next, and nothing more: not whether the system holds the band,
    /// not its signal, and never this Mac's id for it, which the window never
    /// shows (D2).
    #[test]
    fn a_band_is_tried_by_its_name_alone() {
        for (held, rssi) in [(true, None), (false, Some(-61)), (false, None)] {
            assert_eq!(
                band("a", held, rssi, Does::Refuse).trying(),
                "found band a, connecting and checking the key"
            );
        }
        let id = "01234567-89ab-4cde-8f01-23456789abcd";
        let mut nameless = band(id, true, None, Does::Refuse);
        nameless.name = None;
        assert_eq!(nameless.name(), "a band with no name");
        assert_eq!(
            nameless.trying(),
            "found a band with no name, connecting and checking the key"
        );
    }

    #[test]
    fn a_band_is_named_like_one() {
        assert!(named_like_a_band("Xiaomi Smart Band 7 5301"));
        assert!(!named_like_a_band("LIFT"));
        assert!(!named_like_a_band("xiaomi smart band"), "the band writes its name one way");
    }
}
