use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::action::Started;
use crate::ble;
use crate::config::{AuthKey, Config, Device, Settings, Setup};
use crate::crypto::b163::KeyPair;
use crate::error::BandError;
use crate::event::{AppChange, BandEvent, Change, Press};
use crate::ipc::{Answer, KeyText};
use crate::pair::{self, Asked, Seen};
use crate::protocol::ProtocolError;
use crate::protocol::auth::{AuthStep, Authenticator, ENDPOINT_AUTH};
use crate::protocol::battery::{self, Battery};
use crate::session::Session;
use btleplug::api::{Central as _, Peripheral as _, ScanFilter};
use btleplug::platform::Peripheral;
use serde::Serialize;
use tokio::sync::{mpsc, oneshot};

/// What the user wants: the link, or no link.
///
/// It is read from where the connection stands, by [`Standing::desired`], and
/// only a person's ask moves that toward Connected: see [`Standing::after`].
/// A timeout, a scan failure or a dropped link can only give up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Desired {
    Connected,
    Disconnected,
}

/// What the radio is actually doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnState {
    Idle,
    Scanning,
    Authenticating,
    Ready,
    Backoff { until: Instant, attempt: u32 },
}

impl fmt::Display for ConnState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnState::Idle => f.write_str("disconnected"),
            ConnState::Scanning => f.write_str("scanning"),
            ConnState::Authenticating => f.write_str("authenticating"),
            ConnState::Ready => f.write_str("ready"),
            ConnState::Backoff { attempt, .. } => write!(f, "backoff (attempt {attempt})"),
        }
    }
}

/// What the loop should do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Start a scan, connect, and authenticate.
    BeginConnect,
    /// Drop the link and go idle.
    Teardown,
    /// Sleep out a retry delay, interruptible by a command.
    Wait(Duration),
    /// Read the next message from a live session.
    Pump,
    /// Do nothing on the radio. Wait for a command only.
    Park,
}

/// Reconcile what the user wants against what the radio is doing.
///
/// Pure, so every transition is testable without Bluetooth.
pub fn next_action(desired: Desired, state: &ConnState, now: Instant) -> Action {
    match desired {
        // While disconnected the supervisor does NO radio work. An expired
        // backoff must not reconnect: that is what makes a manual disconnect
        // stick, and what stops us starving the user's phone of the band.
        Desired::Disconnected => match state {
            ConnState::Idle | ConnState::Backoff { .. } => Action::Park,
            ConnState::Ready | ConnState::Scanning | ConnState::Authenticating => Action::Teardown,
        },
        Desired::Connected => match state {
            ConnState::Idle => Action::BeginConnect,
            ConnState::Backoff { until, .. } => {
                if now >= *until {
                    Action::BeginConnect
                } else {
                    Action::Wait(*until - now)
                }
            }
            // Scanning and Authenticating are transient: connect_once holds
            // them and the loop does not observe them at a decision point.
            ConnState::Ready | ConnState::Scanning | ConnState::Authenticating => Action::Pump,
        },
    }
}

/// Retry delay for the given attempt. attempt 0 means no delay.
pub fn backoff_delay(attempt: u32) -> Duration {
    if attempt == 0 {
        return Duration::from_secs(0);
    }
    let shift = (attempt - 1).min(20);
    let secs = 2u64.saturating_mul(1u64 << shift);
    Duration::from_secs(secs.min(30))
}

/// Where the connection stands, in a person's terms: the state the window
/// shows, why it began, and when.
///
/// [`ConnState`] is the radio's view, and it passes through scanning,
/// authenticating and backoff on every retry. This moves only when something
/// a person would notice happens: the link comes up, drops or fails, or a
/// person asks for something. So `since` is the moment the window names: when
/// the link came up, when it dropped, when the person let the band go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Standing {
    pub state: Phase,
    pub reason: Reason,
    /// Sent as local wall-clock time, see [`local_time`].
    #[serde(serialize_with = "as_local_time")]
    pub since: SystemTime,
}

/// The states behind the design's five connection cards. Not connected is
/// one state with two reasons: a person let the band go, or the link dropped
/// and nothing tries again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// The link is up.
    Connected,
    /// A connect runs, and none has failed since it began.
    Connecting,
    /// The band went away, or could not be reached, and BandBridge tries
    /// again after a pause.
    Retrying,
    /// Nothing tries. The band stays away until a person asks for it.
    Disconnected,
}

/// Why the connection stands where it does: what began the state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// BandBridge started, and nothing has happened since.
    Started,
    /// A person asked: connect, disconnect, cancel, pair, forget or install.
    You,
    /// The link was up, and went down by itself.
    Dropped,
    /// An attempt to connect did not reach the band, or reached another one.
    Failed,
}

/// Something that moves where the connection stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Happened {
    /// A person asked for the link: Connect, Reconnect, or a pair.
    AskedToConnect,
    /// A person asked to let the band go: Disconnect, Cancel while
    /// connecting, a forget, or quitting.
    AskedToDisconnect,
    /// A person asked to stop the retries: Stop trying, or auto reconnect
    /// turned off while BandBridge tries again after a drop.
    AskedToStopTrying,
    /// The link came up.
    LinkUp,
    /// The link went down by itself. `retry` is the auto reconnect setting.
    LinkLost { retry: bool },
    /// An attempt to connect failed. `retry` says whether another follows.
    AttemptFailed { retry: bool },
    /// BandBridge closed the link to open it again at once. An install does
    /// this, because it spends the session it used.
    Reopening,
}

impl Standing {
    /// Where a supervisor starts: connecting when it has a band to reach, and
    /// waiting for a person when it has none.
    pub fn at_start(can_connect: bool, now: SystemTime) -> Self {
        let state = if can_connect {
            Phase::Connecting
        } else {
            Phase::Disconnected
        };
        Self {
            state,
            reason: Reason::Started,
            since: now,
        }
    }

    /// Where the connection stands after this has happened, at `now`.
    ///
    /// Pure, so every reason is testable without Bluetooth. Only a person's
    /// ask for the link leaves Disconnected. A drop, a failed attempt, or a
    /// link that comes up anyway never does. That is what makes a disconnect
    /// stick, and what keeps the band free for the person's phone.
    pub fn after(self, happened: Happened, now: SystemTime) -> Self {
        use Phase::{Connected, Connecting, Disconnected, Retrying};
        let to = |state, reason| Self {
            state,
            reason,
            since: now,
        };
        match (self.state, happened) {
            (Disconnected, Happened::AskedToConnect) => to(Connecting, Reason::You),
            (Disconnected, _) => self,

            (_, Happened::AskedToDisconnect) => to(Disconnected, Reason::You),

            // A connect the person asks for replaces a retry. A connect that
            // runs, or a link that is up, is what they asked for already.
            (Retrying, Happened::AskedToConnect) => to(Connecting, Reason::You),
            (Connected | Connecting, Happened::AskedToConnect) => self,

            // The band stays away for the reason it went, since the moment it
            // went. Only a retry has anything to stop.
            (Retrying, Happened::AskedToStopTrying) => Self {
                state: Disconnected,
                ..self
            },
            (_, Happened::AskedToStopTrying) => self,

            // Up since now, for the reason the link was sought.
            (_, Happened::LinkUp) => Self {
                state: Connected,
                since: now,
                ..self
            },

            (_, Happened::LinkLost { retry: true }) => to(Retrying, Reason::Dropped),
            (_, Happened::LinkLost { retry: false }) => to(Disconnected, Reason::Dropped),

            // A retry that fails is the same retry: the band has been away
            // since it first went.
            (Retrying, Happened::AttemptFailed { retry: true }) => self,
            (_, Happened::AttemptFailed { retry: true }) => to(Retrying, Reason::Failed),
            (_, Happened::AttemptFailed { retry: false }) => to(Disconnected, Reason::Failed),

            (_, Happened::Reopening) => to(Connecting, Reason::You),
        }
    }

    /// What the radio should do: keep or seek the link, everywhere but
    /// disconnected.
    pub fn desired(&self) -> Desired {
        match self.state {
            Phase::Disconnected => Desired::Disconnected,
            _ => Desired::Connected,
        }
    }
}

impl Phase {
    /// The word the wire and the terminal use.
    pub fn word(self) -> &'static str {
        match self {
            Phase::Connected => "connected",
            Phase::Connecting => "connecting",
            Phase::Retrying => "retrying",
            Phase::Disconnected => "disconnected",
        }
    }
}

impl Reason {
    /// The word the wire and the terminal use.
    pub fn word(self) -> &'static str {
        match self {
            Reason::Started => "started",
            Reason::You => "you",
            Reason::Dropped => "dropped",
            Reason::Failed => "failed",
        }
    }
}

/// For the terminal: `retrying (dropped) since 2026-09-24T21:02:13-04:00`.
/// The window words the same facts itself.
impl fmt::Display for Standing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}) since {}",
            self.state.word(),
            self.reason.word(),
            local_time(self.since)
        )
    }
}

/// A moment as local wall-clock time, to the second, with its offset from
/// UTC: RFC 3339, such as `2026-09-24T21:02:13-04:00`. A page reads it with
/// `new Date(text)`, and a person can read it on the socket.
pub fn local_time(at: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(at).to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

/// A moment as [`local_time`] writes it, for a field that serde sends.
pub(crate) fn as_local_time<S: serde::Serializer>(
    at: &SystemTime,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&local_time(*at))
}

/// What the supervisor says when asked to connect, and the file names no
/// band.
const NOTHING_TO_CONNECT: &str =
    "no band is paired, so there is nothing to connect to. Pair a band first.";

/// What it says when the file names a band, and the store holds no key for
/// it. The window says it too, as it starts with no key for the paired band,
/// so one fault reads one way. It names no environment variable:
/// `BAND_AUTH_KEY` is a developer's override, and never the way out for a
/// person.
///
/// Its advice to forget is safe only here: the store holds no key, so a
/// forget loses nothing. A store that could not be read gives its own reason
/// instead, since the key may well be in it.
pub const NO_KEY_TO_CONNECT: &str = concat!(
    crate::platform::this_computer!(),
    " has no auth key for this band, so it cannot connect. Forget the band, then pair it again \
     with the key from your account login."
);

/// What a Connect answers, before the file's own reason, when the file does
/// not load. It never says that no band is paired: one may well be.
const FILE_UNREADABLE: &str =
    "the config file could not be read, so Mi Band Bridge cannot connect until it is fixed";

/// How the supervisor gets the key for a band's MAC: [`AuthKey::load`], which
/// tries `BAND_AUTH_KEY`, then the system's secret store. A test gives one
/// that reaches no store.
type KeyLoader = fn(Option<&str>) -> Result<AuthKey, BandError>;

/// What it says when it takes up a setting that changed in the file.
const AUTO_RECONNECT_ON: &str = "auto reconnect is on: a link that drops is tried again";
const AUTO_RECONNECT_OFF: &str = "auto reconnect is off: a link that drops stays down";
const NEW_APP_ON_TOP_ON: &str =
    "new app on top is on: an app installed here goes first in the app menu";
const NEW_APP_ON_TOP_OFF: &str =
    "new app on top is off: an app installed here goes where the band puts it";
/// And when that ends a retry after a drop.
const STOPPED_FOR_AUTO_RECONNECT: &str =
    "stopped trying, because auto reconnect is off; the band stays disconnected";

/// What a file looked like at one look: when it was last written, and how
/// long it was. The length tells two writes apart that a coarse clock gives
/// the same time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    modified: SystemTime,
    len: u64,
}

impl FileStamp {
    /// The file as it is now, or None when it cannot be read, which includes
    /// a file that is not there.
    fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Self {
            modified: meta.modified().ok()?,
            len: meta.len(),
        })
    }
}

#[cfg(test)]
mod decision_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn connects_when_wanted_and_idle() {
        let now = Instant::now();
        assert_eq!(
            next_action(Desired::Connected, &ConnState::Idle, now),
            Action::BeginConnect
        );
    }

    #[test]
    fn pumps_while_ready_and_wanted() {
        let now = Instant::now();
        assert_eq!(
            next_action(Desired::Connected, &ConnState::Ready, now),
            Action::Pump
        );
    }

    #[test]
    fn waits_out_a_backoff_then_retries() {
        let now = Instant::now();
        let state = ConnState::Backoff {
            until: now + Duration::from_secs(5),
            attempt: 2,
        };
        match next_action(Desired::Connected, &state, now) {
            Action::Wait(d) => assert!(d <= Duration::from_secs(5) && d > Duration::from_secs(4)),
            other => panic!("expected Wait, got {other:?}"),
        }

        let elapsed = now + Duration::from_secs(6);
        assert_eq!(
            next_action(Desired::Connected, &state, elapsed),
            Action::BeginConnect
        );
    }

    #[test]
    fn tears_down_a_live_link_when_not_wanted() {
        let now = Instant::now();
        for state in [ConnState::Ready, ConnState::Scanning, ConnState::Authenticating] {
            assert_eq!(
                next_action(Desired::Disconnected, &state, now),
                Action::Teardown,
                "failed for {state:?}"
            );
        }
    }

    /// The hard requirement: while disconnected the supervisor does no radio work.
    #[test]
    fn parks_and_never_scans_while_disconnected() {
        let now = Instant::now();
        assert_eq!(next_action(Desired::Disconnected, &ConnState::Idle, now), Action::Park);

        let expired = ConnState::Backoff {
            until: now - Duration::from_secs(10),
            attempt: 3,
        };
        assert_eq!(
            next_action(Desired::Disconnected, &expired, now),
            Action::Park,
            "an expired backoff must not reconnect while disconnected"
        );
    }

    #[test]
    fn backoff_doubles_and_caps_at_thirty_seconds() {
        assert_eq!(backoff_delay(1), Duration::from_secs(2));
        assert_eq!(backoff_delay(2), Duration::from_secs(4));
        assert_eq!(backoff_delay(3), Duration::from_secs(8));
        assert_eq!(backoff_delay(4), Duration::from_secs(16));
        assert_eq!(backoff_delay(5), Duration::from_secs(30));
        assert_eq!(backoff_delay(6), Duration::from_secs(30));
        assert_eq!(backoff_delay(40), Duration::from_secs(30));
    }

    #[test]
    fn states_render_for_the_status_line() {
        assert_eq!(ConnState::Idle.to_string(), "disconnected");
        assert_eq!(ConnState::Ready.to_string(), "ready");
        assert_eq!(ConnState::Scanning.to_string(), "scanning");
        assert_eq!(ConnState::Authenticating.to_string(), "authenticating");
        let backoff = ConnState::Backoff {
            until: Instant::now() + Duration::from_secs(8),
            attempt: 3,
        };
        assert_eq!(backoff.to_string(), "backoff (attempt 3)");
    }
}

/// One test or more for each reason a state begins, and for what the radio
/// does next in it.
#[cfg(test)]
mod standing_tests {
    use super::*;
    use std::time::{Duration, Instant, UNIX_EPOCH};

    /// A moment `secs` seconds into the test's own clock.
    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_790_298_000 + secs)
    }

    fn standing(state: Phase, reason: Reason, since: SystemTime) -> Standing {
        Standing {
            state,
            reason,
            since,
        }
    }

    const EVERY_HAPPENING: [Happened; 9] = [
        Happened::AskedToConnect,
        Happened::AskedToDisconnect,
        Happened::AskedToStopTrying,
        Happened::LinkUp,
        Happened::LinkLost { retry: true },
        Happened::LinkLost { retry: false },
        Happened::AttemptFailed { retry: true },
        Happened::AttemptFailed { retry: false },
        Happened::Reopening,
    ];

    // --- started ----------------------------------------------------------

    /// A paired band is sought as soon as BandBridge starts. With none, it
    /// waits, and does no radio work.
    #[test]
    fn it_starts_connecting_to_a_paired_band_and_waits_with_none() {
        let paired = Standing::at_start(true, at(0));
        assert_eq!(paired, standing(Phase::Connecting, Reason::Started, at(0)));
        assert_eq!(paired.desired(), Desired::Connected);
        assert_eq!(
            next_action(paired.desired(), &ConnState::Idle, Instant::now()),
            Action::BeginConnect
        );

        let unpaired = Standing::at_start(false, at(0));
        assert_eq!(unpaired, standing(Phase::Disconnected, Reason::Started, at(0)));
        assert_eq!(
            next_action(unpaired.desired(), &ConnState::Idle, Instant::now()),
            Action::Park
        );
    }

    /// The link that the start sought comes up, and says why it was sought.
    #[test]
    fn a_link_is_connected_since_it_came_up() {
        let up = Standing::at_start(true, at(0)).after(Happened::LinkUp, at(4));
        assert_eq!(up, standing(Phase::Connected, Reason::Started, at(4)));

        let asked = standing(Phase::Connecting, Reason::You, at(10));
        assert_eq!(
            asked.after(Happened::LinkUp, at(13)),
            standing(Phase::Connected, Reason::You, at(13))
        );
    }

    // --- you --------------------------------------------------------------

    /// Connect, and Reconnect after a drop: a fresh connect, since the ask.
    #[test]
    fn you_connect_from_disconnected_or_from_a_retry() {
        for reason in [Reason::Started, Reason::You, Reason::Dropped, Reason::Failed] {
            let away = standing(Phase::Disconnected, reason, at(1));
            assert_eq!(
                away.after(Happened::AskedToConnect, at(5)),
                standing(Phase::Connecting, Reason::You, at(5)),
                "from disconnected ({reason:?})"
            );
        }
        let retrying = standing(Phase::Retrying, Reason::Dropped, at(1));
        assert_eq!(
            retrying.after(Happened::AskedToConnect, at(5)),
            standing(Phase::Connecting, Reason::You, at(5))
        );
    }

    /// A link that is up, or a connect that runs, is what the ask wants
    /// already. Neither starts over.
    #[test]
    fn a_connect_while_connected_or_connecting_changes_nothing() {
        for state in [Phase::Connected, Phase::Connecting] {
            let now = standing(state, Reason::Started, at(1));
            assert_eq!(now.after(Happened::AskedToConnect, at(9)), now, "{state:?}");
        }
    }

    /// Disconnect while connected, and Cancel while connecting or retrying.
    #[test]
    fn you_disconnect_or_cancel_from_every_state_but_disconnected() {
        for (state, reason) in [
            (Phase::Connected, Reason::You),
            (Phase::Connecting, Reason::Started),
            (Phase::Retrying, Reason::Dropped),
            (Phase::Retrying, Reason::Failed),
        ] {
            let now = standing(state, reason, at(1));
            let after = now.after(Happened::AskedToDisconnect, at(7));
            assert_eq!(after, standing(Phase::Disconnected, Reason::You, at(7)), "{state:?}");
            assert_eq!(
                next_action(after.desired(), &ConnState::Idle, Instant::now()),
                Action::Park
            );
        }
        // A second Disconnect keeps the moment of the first.
        let away = standing(Phase::Disconnected, Reason::You, at(7));
        assert_eq!(away.after(Happened::AskedToDisconnect, at(20)), away);
    }

    /// An install spends its session, and the link opens again at once.
    #[test]
    fn an_install_reconnects_as_you_asked() {
        let up = standing(Phase::Connected, Reason::Started, at(1));
        let after = up.after(Happened::Reopening, at(30));
        assert_eq!(after, standing(Phase::Connecting, Reason::You, at(30)));
        assert_eq!(after.desired(), Desired::Connected);
    }

    // --- dropped ----------------------------------------------------------

    /// Auto reconnect on: retrying, since the drop, for as long as the
    /// retries fail.
    #[test]
    fn a_drop_with_auto_reconnect_on_retries_since_the_drop() {
        let up = standing(Phase::Connected, Reason::You, at(1));
        let dropped = up.after(Happened::LinkLost { retry: true }, at(60));
        assert_eq!(dropped, standing(Phase::Retrying, Reason::Dropped, at(60)));
        assert_eq!(dropped.desired(), Desired::Connected);

        let failed_again = dropped
            .after(Happened::AttemptFailed { retry: true }, at(90))
            .after(Happened::AttemptFailed { retry: true }, at(130));
        assert_eq!(failed_again, dropped, "a failed retry moved the moment of the drop");

        // The retries wait out their pause, then connect.
        let now = Instant::now();
        let backoff = ConnState::Backoff {
            until: now + Duration::from_secs(4),
            attempt: 2,
        };
        assert!(matches!(
            next_action(failed_again.desired(), &backoff, now),
            Action::Wait(_)
        ));

        // A retry that works: connected since then, after a drop.
        assert_eq!(
            failed_again.after(Happened::LinkUp, at(140)),
            standing(Phase::Connected, Reason::Dropped, at(140))
        );
    }

    /// Auto reconnect off: connection lost, since the drop, and no retry.
    #[test]
    fn a_drop_with_auto_reconnect_off_is_lost_and_stays_down() {
        let up = standing(Phase::Connected, Reason::You, at(1));
        let lost = up.after(Happened::LinkLost { retry: false }, at(60));
        assert_eq!(lost, standing(Phase::Disconnected, Reason::Dropped, at(60)));
        assert_eq!(
            next_action(lost.desired(), &ConnState::Idle, Instant::now()),
            Action::Park
        );
    }

    /// Stop trying turns a retry after a drop into a drop with auto
    /// reconnect off: the band stays away, since the drop.
    #[test]
    fn stop_trying_ends_the_retries_and_keeps_the_drop() {
        let retrying = standing(Phase::Retrying, Reason::Dropped, at(60));
        let stopped = retrying.after(Happened::AskedToStopTrying, at(75));
        assert_eq!(stopped, standing(Phase::Disconnected, Reason::Dropped, at(60)));

        // Nothing tries, even once a pending pause is over.
        let expired = ConnState::Backoff {
            until: Instant::now() - Duration::from_secs(1),
            attempt: 3,
        };
        assert_eq!(next_action(stopped.desired(), &expired, Instant::now()), Action::Park);

        // After a failed first connect, the band stays away since then.
        let failing = standing(Phase::Retrying, Reason::Failed, at(10));
        assert_eq!(
            failing.after(Happened::AskedToStopTrying, at(75)),
            standing(Phase::Disconnected, Reason::Failed, at(10))
        );
    }

    /// Stop trying has nothing to stop but a retry. A band that came back a
    /// moment before the ask stays connected.
    #[test]
    fn stop_trying_leaves_every_other_state_alone() {
        for now in [
            standing(Phase::Connected, Reason::Dropped, at(1)),
            standing(Phase::Connecting, Reason::You, at(1)),
            standing(Phase::Disconnected, Reason::You, at(1)),
        ] {
            assert_eq!(now.after(Happened::AskedToStopTrying, at(9)), now);
        }
    }

    // --- failed -----------------------------------------------------------

    /// A first attempt that fails is retried, since the failure.
    #[test]
    fn a_failed_connect_retries_since_it_failed() {
        let connecting = Standing::at_start(true, at(0));
        let failed = connecting.after(Happened::AttemptFailed { retry: true }, at(20));
        assert_eq!(failed, standing(Phase::Retrying, Reason::Failed, at(20)));
        assert_eq!(failed.desired(), Desired::Connected);
    }

    /// A band that is not the paired one stops the retries, whatever the
    /// person wanted before, and the loop then does no radio work.
    #[test]
    fn a_wrong_band_stops_the_retries() {
        for now in [
            standing(Phase::Connecting, Reason::You, at(1)),
            standing(Phase::Retrying, Reason::Dropped, at(1)),
        ] {
            let stopped = now.after(Happened::AttemptFailed { retry: false }, at(9));
            assert_eq!(stopped, standing(Phase::Disconnected, Reason::Failed, at(9)));
            assert_eq!(
                next_action(stopped.desired(), &ConnState::Idle, Instant::now()),
                Action::Park
            );
        }
    }

    // --- the rule under all of them --------------------------------------

    /// Only a person's ask for the link leaves disconnected. A drop, a
    /// failed or finished connect, or an install never revives it, whatever
    /// the auto reconnect setting, and the moment and the reason stay.
    #[test]
    fn a_disconnect_is_never_revived_but_by_a_person() {
        for reason in [Reason::Started, Reason::You, Reason::Dropped, Reason::Failed] {
            let away = standing(Phase::Disconnected, reason, at(1));
            for happened in EVERY_HAPPENING {
                let after = away.after(happened, at(50));
                if happened == Happened::AskedToConnect {
                    assert_eq!(after.state, Phase::Connecting);
                } else {
                    assert_eq!(after, away, "{happened:?} moved a disconnect ({reason:?})");
                }
            }
        }
    }

    // --- the wire and the terminal ----------------------------------------

    /// The window gets three facts, and words them itself. The time is local,
    /// with its offset, to the second.
    #[test]
    fn a_standing_is_sent_as_state_reason_and_local_time() {
        let since = at(133);
        let json = serde_json::to_value(standing(Phase::Retrying, Reason::Dropped, since)).unwrap();
        let object = json.as_object().unwrap();
        assert_eq!(object.len(), 3, "{json}");
        assert_eq!(json["state"], "retrying");
        assert_eq!(json["reason"], "dropped");

        let text = json["since"].as_str().unwrap();
        let read = chrono::DateTime::parse_from_rfc3339(text).unwrap();
        let secs = since.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        assert_eq!(read.timestamp(), secs, "{text}");
        let local = chrono::DateTime::<chrono::Local>::from(since);
        assert_eq!(read.offset().local_minus_utc(), local.offset().local_minus_utc());
        assert!(!text.contains('.'), "to the second: {text}");
    }

    /// Each word on the wire is the word the terminal shows.
    #[test]
    fn the_words_are_the_same_on_the_wire_and_in_the_terminal() {
        for state in [Phase::Connected, Phase::Connecting, Phase::Retrying, Phase::Disconnected] {
            assert_eq!(serde_json::to_value(state).unwrap(), state.word());
        }
        for reason in [Reason::Started, Reason::You, Reason::Dropped, Reason::Failed] {
            assert_eq!(serde_json::to_value(reason).unwrap(), reason.word());
        }
        let shown = standing(Phase::Disconnected, Reason::You, at(0)).to_string();
        assert_eq!(shown, format!("disconnected (you) since {}", local_time(at(0))));
    }
}

/// What `pump` woke up for. Keeps the two borrows in `pump` disjoint.
enum Next {
    Command(Option<Command>),
    Request(Option<crate::ipc::Incoming>),
    Message(Result<crate::protocol::chunk::ZeppMessage, BandError>),
}

/// The longest an install may take before it is called a failure.
///
/// Without this a stuck link leaves the caller waiting with nothing to show.
const INSTALL_LIMIT: Duration = Duration::from_secs(180);

/// How long to wait for each handshake reply.
const AUTH_TIMEOUT: Duration = Duration::from_secs(15);
/// How long to wait for the system to confirm it let a band go.
const RELEASE_LIMIT: Duration = Duration::from_secs(5);
/// A quiet link is normal. This only bounds one read.
const READ_TIMEOUT: Duration = Duration::from_secs(300);

/// An instruction for the supervisor, from a person.
///
/// Only the front end that runs the supervisor sends these: the window, or
/// `listen`. The socket carries requests, never a command.
#[derive(Debug)]
pub enum Command {
    Connect,
    /// Let the band go, and stay disconnected. Cancel while connecting is this
    /// too: the connect runs to its end, and lets the band go when it lands,
    /// because stopping it part way would skip the cleanup that releases it.
    Disconnect,
    /// Stop the retries after a drop, and stay disconnected. Anything but a
    /// retry is left as it is, so a band that came back a moment before
    /// stays connected.
    StopTrying,
    /// Ask the supervisor to re-emit its current state.
    ReportState,
    /// The config file changed. Take up what it says now, rather than at the
    /// next look. The window sends this after it writes the file.
    ConfigChanged,
    /// Learn from the device, in the window's editor: answer the next press
    /// with its signal here, and fire nothing for it. A learn that waits
    /// already is stopped, so one learn runs at a time. See [`learn`].
    Learn(oneshot::Sender<Learned>),
    /// Stop a learn, if one waits. The next press fires as usual.
    StopLearning,
    Shutdown,
}

/// How a learn ended: what the band task answers on [`Command::Learn`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Learned {
    /// The next press came, with this signal, in lower case hex. Nothing
    /// fired for it.
    Signal(String),
    /// The learn was stopped before a press came, or another learn took its
    /// place. There is nothing to show.
    Stopped,
    /// No press can come, and why, in words fit to show: the band is not
    /// connected, or it went away before a press came.
    Failed(String),
}

/// How long Learn from the device waits for a press.
pub const LEARN_LIMIT: Duration = Duration::from_secs(60);

/// Why a learn gave up, after [`LEARN_LIMIT`].
const NO_PRESS: &str = "no press came in a minute, so Learn stopped. Press Learn from the \
     device, then press the button on the band.";

/// Why a learn cannot start: only a connected band sends a press.
const NOT_CONNECTED_TO_LEARN: &str =
    "the band is not connected, so it cannot send a press. Connect it, then try again.";

/// Why a learn ended with no press: the band went away first.
const WENT_BEFORE_A_PRESS: &str = "the band disconnected before a press came";

/// Why a request to the band task went unanswered.
const GONE: &str = "the band task is gone";

/// Learn the signal of the next press, for the window's editor: Learn from
/// the device. The band task answers it, and fires nothing for that press.
///
/// Some(signal) for a press. None when the learn was stopped, or another
/// took its place: there is nothing to show. An error when no press can
/// come, or none came within `limit`, in words fit to show.
///
/// The wait is here, not in the band task, whose loop has no timer (see
/// [`Supervisor::run`]). A learn given up on is closed before it is read a
/// last time, so a press that comes just then either reaches it or fires as
/// usual: it is never lost between the two.
pub async fn learn(
    commands: &mpsc::Sender<Command>,
    limit: Duration,
) -> Result<Option<String>, String> {
    let (learner, mut learned) = oneshot::channel();
    commands
        .send(Command::Learn(learner))
        .await
        .map_err(|_| GONE.to_string())?;
    let answer = match tokio::time::timeout(limit, &mut learned).await {
        Ok(answer) => answer.map_err(|_| GONE.to_string())?,
        Err(_) => {
            learned.close();
            learned.try_recv().map_err(|_| NO_PRESS.to_string())?
        }
    };
    match answer {
        Learned::Signal(signal) => Ok(Some(signal)),
        Learned::Stopped => Ok(None),
        Learned::Failed(why) => Err(why),
    }
}

/// Something the supervisor wants the front end to show.
#[derive(Debug, Clone)]
pub enum Status {
    /// What the radio is doing, and where the connection stands. Sent when
    /// either changes.
    State(ConnState, Standing),
    /// What the band sent, as it came: a press's bytes, and the band's own
    /// traffic. A terminal prints it. The window shows a press as a
    /// [`Status::Pressed`] instead, and the rest not at all.
    Event(BandEvent),
    /// A press came, with the action it fired, if any: a line of the
    /// window's event log. A keystroke has its result already. An action that
    /// takes time is still running, and comes again, with its result, as
    /// [`Status::Finished`].
    Pressed(Press),
    /// The action a press fired has ended: the same press, now with its
    /// result. Always after its [`Status::Pressed`].
    Finished(Press),
    /// An app was installed on the band, or removed from it: a line of the
    /// window's event log.
    AppChanged(AppChange),
    Failed(String),
    Notice(String),
}

impl Status {
    /// The line a terminal prints for this, or None for what a terminal
    /// does not print. `listen` and `follow` both print from here, so the two
    /// never disagree.
    ///
    /// A press reads as it always has: the band's bytes, from its
    /// [`Status::Event`], then `fired` with the action, then the action's
    /// outcome when it ends. A press that fired nothing, and an app installed
    /// or removed, add no line.
    pub fn terminal_line(&self) -> Option<String> {
        match self {
            Status::State(state, standing) => Some(format!("[state] {state} · {standing}")),
            Status::Event(event) => Some(event.to_string()),
            Status::Pressed(press) => {
                let name = press.action.as_deref()?;
                let does = press.does.as_deref().unwrap_or_default();
                Some(format!("[info]   fired {name} ({does})"))
            }
            Status::Finished(press) => {
                let name = press.action.as_deref()?;
                let outcome = press.result.as_ref()?;
                Some(format!("[info]   {name}: {outcome}"))
            }
            Status::AppChanged(_) => None,
            Status::Failed(reason) => Some(format!("[error] {reason}")),
            Status::Notice(text) => Some(format!("[info] {text}")),
        }
    }
}

/// How to reach the band. Set when the run starts, and again when a band is
/// paired or forgotten.
#[derive(Debug, Clone)]
pub struct ConnectOptions {
    pub peripheral_id: Option<String>,
    /// The paired band's MAC. A band that reports another is let go before it
    /// hears the key.
    pub mac: Option<String>,
    pub raw: bool,
    pub mtu: Option<u16>,
    pub subscribe_all: bool,
    pub scan_seconds: u64,
}

/// An open, authenticated link, and what the band said about itself on it.
struct Link {
    session: Session,
    facts: crate::device::Facts,
    /// This computer's id for the band.
    id: String,
}

/// What a connect running in a task of its own is for.
enum Purpose {
    /// Reach the paired band.
    Connect,
    /// Find the band that takes a key, and pair it.
    Pair(PairJob),
}

/// A pair in progress: what the person gave, and who waits for the answer.
struct PairJob {
    key: AuthKey,
    name: String,
    setup: Setup,
    answer: Answer,
}

/// A connect running in its own task, and the states it reports as it goes.
struct Connecting {
    task: tokio::task::JoinHandle<Result<Link, BandError>>,
    progress: mpsc::Receiver<ConnState>,
    purpose: Purpose,
}

/// Open one link and authenticate it, away from the supervisor's loop.
///
/// The radio's progress goes back on `progress`, which moves the supervisor's
/// own state. The supervisor tells the front ends, because only it knows
/// where the connection stands, and a status request is answered from that
/// state while this runs. `status` carries what was found, for the log.
async fn connect_once(
    opts: ConnectOptions,
    auth_key: AuthKey,
    status: mpsc::Sender<Status>,
    progress: mpsc::Sender<ConnState>,
) -> Result<Link, BandError> {
    let adapter = ble::adapter().await?;
    let peripheral = ble::find_band(
        &adapter,
        Duration::from_secs(opts.scan_seconds),
        opts.peripheral_id.as_deref(),
    )
    .await?;
    let name = ble::peripheral_name(&peripheral)
        .await
        .unwrap_or_else(|| "-".into());
    let _ = status.send(Status::Notice(format!("found {name}"))).await;

    open_link(peripheral, auth_key, &opts, &progress).await
}

/// Open a link to this band, and authenticate it with the key.
///
/// What the band says about itself is read first, with plain reads on the
/// standard services, before the Zepp session starts. That is where its MAC
/// comes from, which macOS will not give. When `opts.mac` names the paired
/// band and this band reports another, it is let go before it hears the key.
async fn open_link(
    peripheral: Peripheral,
    auth_key: AuthKey,
    opts: &ConnectOptions,
    progress: &mpsc::Sender<ConnState>,
) -> Result<Link, BandError> {
    let id = peripheral.id().to_string();
    let chunked = ble::connect_and_discover(&peripheral).await?;

    // From here the link is OPEN. Every failure path below must release it,
    // or the band is stranded and the user's phone can never pair.
    let cleanup = peripheral.clone();

    let facts = crate::device::read(&peripheral).await;
    if let Some(paired) = opts.mac.as_deref()
        && let Some(found) = facts.mac.as_deref()
        && !crate::device::same_address(paired, found)
    {
        release(&cleanup).await;
        return Err(BandError::WrongBand {
            paired: paired.to_string(),
            found: found.to_string(),
        });
    }

    let mut session = match Session::open(
        peripheral,
        chunked,
        opts.raw,
        opts.mtu,
        opts.subscribe_all,
    )
    .await
    {
        Ok(session) => session,
        Err(err) => {
            release(&cleanup).await;
            return Err(err);
        }
    };

    let _ = progress.send(ConnState::Authenticating).await;
    if let Err(err) = authenticate(&mut session, auth_key).await {
        // The peripheral, not the session: a shared reference to a session
        // cannot cross a task boundary, and this releases the same link.
        release(&cleanup).await;
        return Err(err);
    }

    Ok(Link { session, facts, id })
}

/// Let go of a band. A Band 7 takes one host at a time, so a link left open
/// starves the person's phone.
async fn release(peripheral: &Peripheral) {
    if let Err(err) = peripheral.disconnect().await {
        tracing::warn!("could not release the band: {err}");
    }
}

/// What a pair says when its scan did not start. The Mac still asks the
/// system for the bands it holds.
#[cfg(target_os = "macos")]
fn no_scan(err: &impl std::fmt::Display) -> String {
    format!("the scan did not start, so only a band this Mac holds can be found: {err}")
}

/// What a pair says when its scan did not start. Windows finds a band by a
/// scan alone (W7), so the pair ends here.
#[cfg(windows)]
fn no_scan(err: &impl std::fmt::Display) -> String {
    format!("the scan did not start, so no band can be found. Turn Bluetooth on, then try again. {err}")
}

/// Find the band in reach that takes this key, and keep its link open.
///
/// A scan runs for the whole search, and the system is asked for the bands it
/// holds as well, because a band macOS holds does not advertise. A scan that
/// cannot start leaves those held bands to ask. The scan stops once a band
/// takes the key, or the search is over.
async fn pair_once(
    opts: ConnectOptions,
    key: AuthKey,
    status: mpsc::Sender<Status>,
    progress: mpsc::Sender<ConnState>,
    answer: Answer,
) -> Result<Link, BandError> {
    let adapter = ble::adapter().await?;
    tell(
        &status,
        &answer,
        format!(
            "looking for bands nearby, for up to {} seconds",
            pair::SEARCH.as_secs()
        ),
    )
    .await;
    let scanning = match adapter.start_scan(ScanFilter::default()).await {
        Ok(()) => true,
        Err(err) => {
            let text = no_scan(&err);
            // On Windows a scan is the only way to find a band (W7), so a scan
            // that will not start ends the pair at once, rather than search on
            // for the whole 25 seconds and find nothing. The Mac still asks
            // the bands it holds, which a scan failing does not keep from it.
            #[cfg(windows)]
            return Err(BandError::Pairing(text));
            #[cfg(target_os = "macos")]
            {
                tell(&status, &answer, text).await;
                false
            }
        }
    };

    let found = pair::first_to_accept(
        std::time::Instant::now() + pair::SEARCH,
        pair::PAUSE,
        || pair::bands_nearby(&adapter),
        |seen| ask_band(seen, &key, &opts, &status, &progress, &answer),
    )
    .await;

    if scanning && let Err(err) = adapter.stop_scan().await {
        tracing::debug!("the scan did not stop: {err}");
    }
    found
}

/// Ask one band for the key: connect, authenticate, and let the band go at
/// once when it refuses.
async fn ask_band(
    seen: Seen<Peripheral>,
    key: &AuthKey,
    opts: &ConnectOptions,
    status: &mpsc::Sender<Status>,
    progress: &mpsc::Sender<ConnState>,
    answer: &Answer,
) -> Asked<Link> {
    let name = seen.name();
    tracing::debug!(
        "trying {name}: held by the system {}, signal {:?}",
        seen.held,
        seen.rssi
    );
    tell(status, answer, seen.trying()).await;
    let peripheral = seen.band;
    let attempt = open_link(peripheral.clone(), key.clone(), opts, progress);
    let asked = match tokio::time::timeout(pair::TRY_LIMIT, attempt).await {
        Ok(Ok(link)) => return Asked::Accepted(link),
        Ok(Err(BandError::Protocol(ProtocolError::AuthRejected))) => {
            tell(status, answer, format!("{name} refused the key")).await;
            Asked::Refused
        }
        Ok(Err(err)) => {
            tell(status, answer, format!("{name} could not be asked: {err}")).await;
            Asked::Failed(err.to_string())
        }
        Err(_) => {
            // The attempt stopped part way, holding whatever it had opened.
            // A link that never opened gives no answer to a release, so the
            // wait for one is bounded.
            let _ = tokio::time::timeout(RELEASE_LIMIT, release(&peripheral)).await;
            let why = format!("it did not answer in {} seconds", pair::TRY_LIMIT.as_secs());
            tell(status, answer, format!("{name} could not be asked: {why}")).await;
            Asked::Failed(why)
        }
    };
    let _ = progress.send(ConnState::Scanning).await;
    asked
}

/// Tell whoever asked for the pair, and the front ends, how it goes.
async fn tell(status: &mpsc::Sender<Status>, answer: &Answer, text: String) {
    answer.line(text.clone()).await;
    let _ = status.send(Status::Notice(text)).await;
}

/// The first action a press fired, as it started: its name, and what it does
/// in a few words, for a terminal's `fired` line.
struct Fired {
    started: Started,
    name: String,
    does: String,
}

/// An action a press fires after the first, waiting for its turn: its name,
/// what it does in a few words, and what it does.
struct Queued {
    name: String,
    shown: String,
    does: crate::config::Does,
}

/// Why a pair cannot keep the band it found: the file names another band,
/// paired while this pair ran.
fn already_paired(device: &Device) -> BandError {
    BandError::Pairing(format!(
        "{} was paired while this pair ran. Forget it before you pair another band.",
        device.name
    ))
}

/// What the event log calls an app installed from this package: the name in
/// its manifest, or the package's file name when no manifest is near it.
fn shown_app_name(package: &Path, name: Option<&str>) -> String {
    if let Some(name) = name {
        return name.to_string();
    }
    match package.file_name() {
        Some(file) => file.to_string_lossy().into_owned(),
        None => package.display().to_string(),
    }
}

/// Tell whoever asked for an install how its upload goes: a line each time
/// the percentage moves. The band app editor reads these as the percentage
/// on its Install button, and a terminal prints them. A caller that reads
/// slowly may miss one, and the next one says more.
fn tell_progress(answer: &crate::ipc::Answer) -> impl FnMut(usize, usize) + Send + 'static {
    let replies = answer.0.clone();
    let mut lines = crate::install::ProgressLines::default();
    move |sent, total| {
        if let Some(text) = lines.line(sent, total) {
            let _ = replies.try_send(crate::ipc::Reply::Line { text });
        }
    }
}

/// The band the config file names, and the key `load` gives for its MAC, or
/// why a Connect cannot have them, in words for a person. It blocks while the
/// file is read, under the config lock, and while the store answers. See
/// [`Supervisor::look_again`].
///
/// - A file that does not load gives [`FILE_UNREADABLE`], then the file's own
///   reason. It never says that no band is paired: one may well be.
/// - A file that names no band gives [`NOTHING_TO_CONNECT`], and no store is
///   asked.
/// - A store with no key for the band gives [`NO_KEY_TO_CONNECT`], which says
///   to forget the band and pair it again: the store holds nothing to lose.
/// - Any other failure, such as a store that could not be read, gives its own
///   reason, and never says to forget: the key may well be in the store, and
///   a forget would delete it.
fn band_and_key(file: Option<&Path>, load: KeyLoader) -> Result<(Device, AuthKey), String> {
    let config = match file {
        Some(path) => Config::load_under_lock(path),
        None => Ok(Config::default()),
    }
    .map_err(|err| {
        let reason = match err {
            BandError::Config(reason) => reason,
            other => other.to_string(),
        };
        format!("{FILE_UNREADABLE}: {reason}")
    })?;
    let Some(device) = config.device else {
        return Err(NOTHING_TO_CONNECT.to_string());
    };
    match load(Some(&device.mac)) {
        Ok(key) => Ok((device, key)),
        Err(BandError::MissingAuthKey(_)) => Err(NO_KEY_TO_CONNECT.to_string()),
        Err(err) => Err(format!("cannot connect: {err}")),
    }
}

pub struct Supervisor {
    /// Where the connection stands. What the radio should do follows from
    /// it: see [`Standing::desired`].
    standing: Standing,
    state: ConnState,
    /// The state and the standing the front ends were last told, so each
    /// change is told once.
    reported: Option<(ConnState, Standing)>,
    attempt: u32,
    /// How BandBridge behaves, as the config file said at the last look.
    settings: Settings,
    /// False for a run that must never reconnect after a drop, whatever the
    /// file says: `listen --no-auto-reconnect`.
    may_reconnect: bool,
    opts: ConnectOptions,
    /// The key for the band. None until a band is paired, and after a forget:
    /// the supervisor then does no radio work but a pair. None too when the
    /// front end could not have it at start: a Connect then looks again, see
    /// [`Supervisor::look_again`].
    auth_key: Option<AuthKey>,
    /// Where that look gets the key: [`AuthKey::load`], unless a test gives a
    /// loader that reaches no store.
    load_key: KeyLoader,
    session: Option<Session>,
    commands: mpsc::Receiver<Command>,
    status: mpsc::Sender<Status>,
    /// Requests from another process, which reaches the band through this one
    /// because a Bluetooth link belongs to whoever opened it.
    requests: Option<mpsc::Receiver<crate::ipc::Incoming>>,
    /// Callers waiting to be told about band events.
    followers: Vec<crate::ipc::Answer>,
    /// The actions to fire. Held here, not in a front end, so a press does
    /// what it says whoever is holding the band.
    actions: Vec<crate::config::Action>,
    /// The config file this supervisor follows: [`Config::path`], unless a
    /// test gives it a file of its own.
    config_file: Option<PathBuf>,
    /// The config file as it was when it was last read.
    ///
    /// Nothing the file holds touches the band. An action is a lookup on this
    /// side, and a setting is read at the moment it is used, so a change needs
    /// no reconnect, let alone a restart. The file is read again when it
    /// changes: see [`Supervisor::follow_changes`].
    config_seen: Option<FileStamp>,
    /// Set after an install from here with New app on top on (D10). The band
    /// puts a new app in the menu wherever it likes, and the lift needs the
    /// transport the install tore down, so it waits for the link to come back.
    lift_when_ready: bool,
    /// The app an install just put on the band, so the lift can put it first
    /// in the menu. The band reports it wherever it likes. Kept only when New
    /// app on top was on as the install ended.
    newest_app: Option<String>,
    /// A connect that is running in a task of its own.
    ///
    /// A scan takes up to twenty seconds. Running it on this loop meant a
    /// window or a command line got no answer for that whole time, which reads
    /// as a hung app. The task reports its progress on a channel, so the state
    /// this loop holds stays true while it runs.
    connecting: Option<Connecting>,
    /// What the band says about itself, read once per link.
    facts: crate::device::Facts,
    /// What the band last said about its battery, and when it said it.
    ///
    /// Held rather than fetched on demand. A fetch would have to drain the
    /// band's messages while it waits, and a press arriving in that window
    /// would be lost. The band answers on the ordinary message path instead.
    battery: Option<(Battery, Instant)>,
    /// How many records the window's event log has been sent, so each new
    /// one takes the next number.
    records: u64,
    /// A learn waiting for the next press, which it takes: the press's
    /// signal goes back on it, and nothing fires. See [`Command::Learn`].
    learner: Option<oneshot::Sender<Learned>>,
}

impl Supervisor {
    /// A supervisor for the band this key opens. With no key, no band is
    /// paired: it waits, answers requests, and connects once a pair finds
    /// the band.
    ///
    /// It fires nothing, and behaves as the default settings say, until it
    /// is given the config it follows: see [`Supervisor::following`].
    pub fn new(
        auth_key: Option<AuthKey>,
        opts: ConnectOptions,
        commands: mpsc::Receiver<Command>,
        status: mpsc::Sender<Status>,
    ) -> Self {
        let standing = Standing::at_start(auth_key.is_some(), SystemTime::now());
        Self {
            standing,
            state: ConnState::Idle,
            // Nothing to tell yet. A front end asks for the state when it
            // opens, and each change after that is told as it happens.
            reported: Some((ConnState::Idle, standing)),
            attempt: 0,
            settings: Settings::default(),
            may_reconnect: true,
            opts,
            auth_key,
            load_key: AuthKey::load,
            session: None,
            commands,
            status,
            requests: None,
            followers: Vec::new(),
            lift_when_ready: false,
            newest_app: None,
            connecting: None,
            facts: crate::device::Facts::default(),
            battery: None,
            records: 0,
            learner: None,
            actions: Vec::new(),
            config_file: Config::path(),
            config_seen: None,
        }
    }

    /// Fire the config's actions, and behave as its settings say. From here
    /// the supervisor follows the file: a change to an action or a setting
    /// counts from the next thing that happens, with no restart.
    ///
    /// The config is the one the front end read at start, so a file that
    /// did not load is the defaults here, and a later fix is a change.
    pub fn following(mut self, config: &Config) -> Self {
        self.actions = config.actions.clone();
        self.settings = config.settings;
        self.config_seen = self.config_stamp();
        self
    }

    /// Never reconnect after a drop in this run, when `never` is true,
    /// whatever the file says: `listen --no-auto-reconnect`.
    pub fn never_reconnecting(mut self, never: bool) -> Self {
        self.may_reconnect = !never;
        self
    }

    /// Whether a link that drops is tried again: as the file says, unless
    /// this run never reconnects.
    fn auto_reconnect(&self) -> bool {
        self.settings.auto_reconnect && self.may_reconnect
    }

    /// The config file as it is now, if there is one to follow.
    fn config_stamp(&self) -> Option<FileStamp> {
        FileStamp::of(self.config_file.as_deref()?)
    }

    /// Take up what the config file says now, when it has changed since the
    /// last look: the actions, auto reconnect and new app on top.
    ///
    /// A look is one stat, so it is taken often: on every pass of the loop,
    /// and again at the moment each of them is used, which is a press, a
    /// drop, and a connect. So a change made in the window, or in the file by
    /// hand, counts from the next thing that happens.
    async fn follow_changes(&mut self) {
        let Some(path) = self.config_file.clone() else {
            return;
        };
        let Some(stamp) = FileStamp::of(&path) else {
            return;
        };
        if self.config_seen == Some(stamp) {
            return;
        }
        // Taken before the read, so a write that lands during the read is
        // seen at the next look.
        self.config_seen = Some(stamp);

        // The loader refuses the whole file for one bad action, rather than
        // half apply it: a list that fires some of its entries is harder to
        // understand than one that fires none. What is held stays.
        match Config::load_from(&path) {
            Ok(config) => self.take_up(&config).await,
            Err(err) => {
                self.send(Status::Notice(format!(
                    "the config file was not loaded, so the actions and settings stay as they \
                     were: {err}"
                )))
                .await;
            }
        }
    }

    /// Take up the actions and the settings a config holds, and say which of
    /// them changed. Appearance is the window's alone.
    async fn take_up(&mut self, config: &Config) {
        if config.actions != self.actions {
            self.actions = config.actions.clone();
            self.send(Status::Notice(format!("{} action(s) loaded", self.actions.len())))
                .await;
        }

        let reconnected = self.auto_reconnect();
        let tidied = self.settings.new_app_on_top;
        self.settings = config.settings;

        if self.auto_reconnect() != reconnected {
            if self.auto_reconnect() {
                // For the next drop. A band that is away stays away: only a
                // person's ask for the link leaves disconnected.
                self.send(Status::Notice(AUTO_RECONNECT_ON.to_string()))
                    .await;
            } else {
                self.send(Status::Notice(AUTO_RECONNECT_OFF.to_string()))
                    .await;
                // A retry after a drop is the reconnect this setting governs,
                // so it ends, as Stop trying ends it. A first connect that
                // failed is not a drop, and goes on.
                let after_a_drop = self.standing.state == Phase::Retrying
                    && self.standing.reason == Reason::Dropped;
                if after_a_drop {
                    self.stop_retrying(STOPPED_FOR_AUTO_RECONNECT).await;
                }
            }
        }
        // For the next install from here. A connect never touches the menu.
        if self.settings.new_app_on_top != tidied {
            let said = if self.settings.new_app_on_top {
                NEW_APP_ON_TOP_ON
            } else {
                NEW_APP_ON_TOP_OFF
            };
            self.send(Status::Notice(said.to_string())).await;
        }
    }

    /// Serve requests from other processes on this band link.
    pub fn serving(mut self, requests: mpsc::Receiver<crate::ipc::Incoming>) -> Self {
        self.requests = Some(requests);
        self
    }

    /// Do what another process asked, on this band link.
    ///
    /// Returning false stops the supervisor, which no request does.
    async fn serve_request(&mut self, incoming: Option<crate::ipc::Incoming>) -> bool {
        use crate::ipc::Request;

        let Some(incoming) = incoming else {
            // Nobody is listening on the socket any more. The band link is not
            // affected, so carry on without it.
            self.requests = None;
            return true;
        };
        let answer = Answer(incoming.reply);

        match incoming.request {
            // The radio's word for a person, then where the connection
            // stands, as data for a window.
            Request::Status => {
                answer.line(format!("state {}", self.state)).await;
                let json = self.standing_json();
                let _ = answer.0.send(crate::ipc::Reply::Data { json }).await;
                answer.done().await;
            }
            Request::Connect => match self.ask_to_connect().await {
                Ok(()) => {
                    answer.line("connecting").await;
                    answer.done().await;
                }
                Err(reason) => answer.failed(reason).await,
            },
            Request::Pair { key, name, setup } => self.begin_pair(key, name, setup, answer).await,
            Request::Forget { setup } => match self.forget(setup).await {
                Ok(lines) => {
                    for line in lines {
                        answer.line(line).await;
                    }
                    answer.done().await;
                }
                Err(err) => answer.failed(err.to_string()).await,
            },
            Request::SavedSetup => {
                let saved = Config::load().and_then(|config| {
                    serde_json::to_string(&config.saved_setup()).map_err(|err| {
                        BandError::Config(format!("cannot encode the saved setup: {err}"))
                    })
                });
                match saved {
                    Ok(json) => {
                        let _ = answer.0.send(crate::ipc::Reply::Data { json }).await;
                        answer.done().await;
                    }
                    Err(err) => answer.failed(err.to_string()).await,
                }
            }
            // Nothing sends this here: the window writes the clipboard itself,
            // and the socket refuses it. The band task has no clipboard, and
            // hands out nothing.
            Request::CopyKey => {
                answer
                    .failed(
                        "only the Mi Band Bridge window puts the key on the clipboard, with \
                         its Copy key button",
                    )
                    .await;
            }
            Request::Disconnect => {
                self.ask_to_disconnect().await;
                answer.line("disconnecting").await;
                answer.done().await;
            }
            Request::Screens {
                list,
                set,
                hide,
                tidy,
            } => {
                let result = self.serve_screens(&list, set.as_deref(), &hide, tidy).await;
                match result {
                    Ok(lines) => {
                        for line in lines {
                            answer.line(line).await;
                        }
                        answer.done().await;
                    }
                    Err(err) => answer.failed(err.to_string()).await,
                }
            }
            Request::Apps { remove, all } => {
                let result = self.serve_apps(remove.as_deref(), all).await;
                match result {
                    Ok(lines) => {
                        for line in lines {
                            answer.line(line).await;
                        }
                        answer.done().await;
                    }
                    Err(err) => answer.failed(err.to_string()).await,
                }
            }
            Request::Install { file } => {
                // An install needs the transport this session holds, so the
                // session goes away first and comes back afterwards.
                // Use the link this supervisor already holds. Opening a second
                // one from the same process does not work: the band is still
                // held by the first, and the install waits with nothing to show.
                let had_link = self.session.is_some();
                let result = match tokio::time::timeout(
                    INSTALL_LIMIT,
                    self.install_here(&file, &answer),
                )
                .await
                {
                    Ok(result) => result,
                    Err(_) => Err(BandError::Install(
                        "the install did not finish in time".to_string(),
                    )),
                };
                self.installed(result, had_link, &answer).await;
            }
            Request::ScreenList { list } => {
                let result = async {
                    let kind = crate::screens::kind_from_name(&list)?;
                    let Some(session) = self.session.as_mut() else {
                        return Err(BandError::DisplayList(
                            "the band is not connected. Connect first.".to_string(),
                        ));
                    };
                    let entries = crate::screens::read_list(session, kind).await?;
                    serde_json::to_string(&entries).map_err(|err| {
                        BandError::DisplayList(format!("cannot encode the list: {err}"))
                    })
                }
                .await;
                match result {
                    Ok(json) => {
                        let _ = answer.0.send(crate::ipc::Reply::Data { json }).await;
                        answer.done().await;
                    }
                    Err(err) => answer.failed(err.to_string()).await,
                }
            }
            Request::AppList { all } => {
                let result = async {
                    let Some(session) = self.session.as_mut() else {
                        return Err(BandError::Apps(
                            "the band is not connected. Connect first.".to_string(),
                        ));
                    };
                    let shown = crate::apps::list(session, all).await?;
                    // The count of what is left out, so a page can say why the
                    // list is shorter than the band without naming any of it.
                    let held = crate::apps::count(session).await.unwrap_or(shown.len());
                    serde_json::to_string(&serde_json::json!({
                        "apps": shown,
                        "hidden": held.saturating_sub(shown.len()),
                    }))
                    .map_err(|err| BandError::Apps(format!("cannot encode the list: {err}")))
                }
                .await;
                match result {
                    Ok(json) => {
                        let _ = answer.0.send(crate::ipc::Reply::Data { json }).await;
                        answer.done().await;
                    }
                    Err(err) => answer.failed(err.to_string()).await,
                }
            }
            Request::Device => {
                // Answer from what the band last said, and ask again for next
                // time. The reply lands on the message loop a moment later.
                self.ask_battery().await;
                // The name is the person's, and the file is where it
                // changes, so it is read there each time.
                let name = Config::load()
                    .ok()
                    .and_then(|config| config.device)
                    .map(|device| device.name);
                let json = self.device_json(name).to_string();
                let _ = answer.0.send(crate::ipc::Reply::Data { json }).await;
                answer.done().await;
            }
            Request::Follow => {
                answer.line("following band events").await;
                self.followers.push(answer);
            }
        }
        true
    }

    /// Where the connection stands, as the JSON a window reads:
    /// `{"state":"retrying","reason":"dropped","since":"2026-09-24T21:02:13-04:00"}`.
    fn standing_json(&self) -> String {
        serde_json::to_string(&self.standing).unwrap_or_else(|err| {
            tracing::warn!("cannot encode where the connection stands: {err}");
            "null".to_string()
        })
    }

    /// What this host knows about the band, as the `device` answer gives it:
    /// the name the person gave it, what the band said about itself on this
    /// link, its battery, and when the link came up, while it is up.
    fn device_json(&self, name: Option<String>) -> serde_json::Value {
        let battery = self.battery.map(|(state, at)| {
            serde_json::json!({
                "percent": state.percent,
                "charging": state.charging,
                "secondsAgo": at.elapsed().as_secs(),
            })
        });
        let connected_since =
            (self.standing.state == Phase::Connected).then(|| local_time(self.standing.since));
        serde_json::json!({
            "name": name,
            "mac": self.facts.mac,
            "serial": self.facts.serial,
            "hardware": self.facts.hardware,
            "firmware": self.facts.firmware,
            "battery": battery,
            "connectedSince": connected_since,
        })
    }

    /// Tell the band what time it is, once per connect.
    ///
    /// The band takes its clock from whichever host it is paired with, and it
    /// keeps time well enough by itself between those moments. One message on
    /// connect is enough. A face showing an old time is a face that did not
    /// redraw, not a clock that drifted: see the zepp-os-watchface-build
    /// skill.
    async fn set_band_clock(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let command = crate::band_time::set_now();
        if let Err(err) = session
            .write_characteristic(crate::band_time::CHAR_CURRENT_TIME, &command)
            .await
        {
            self.send(Status::Notice(format!("the clock was not set: {err}")))
                .await;
        }
    }

    /// Put the app an install just put on the band first in the app menu, if
    /// this link owes it. See [`Supervisor::lift_owed`].
    ///
    /// The band drops a new app into the menu wherever it likes. The lift needs
    /// the chunked transport, which an install tears down, so it runs here on
    /// the link that replaced it.
    async fn lift_if_owed(&mut self) {
        let Some(newest) = self.lift_owed() else {
            return;
        };
        let Some(session) = self.session.as_mut() else {
            return;
        };
        match crate::screens::lift_to_top(session, &newest).await {
            Ok(lines) => {
                for line in lines {
                    self.send(Status::Notice(line)).await;
                }
            }
            Err(err) => {
                self.send(Status::Notice(format!(
                    "The app is installed. It could not be put first in the menu: {err}"
                )))
                .await;
            }
        }
    }

    /// The app this link puts first in the app menu, if any (D10).
    ///
    /// Only an install from here with New app on top on owes a lift, and only
    /// once. A connect alone leaves the menu as the person arranged it,
    /// whatever the switch says. A switch turned off before the link came
    /// back cancels the lift.
    fn lift_owed(&mut self) -> Option<String> {
        if !std::mem::take(&mut self.lift_when_ready) {
            return None;
        }
        let newest = self.newest_app.take();
        if self.settings.new_app_on_top {
            newest
        } else {
            None
        }
    }

    /// Learn what an app is called, from the manifest beside its package,
    /// and remember it: its id and its name.
    ///
    /// Nothing depends on this working: a nameless app still installs, lists,
    /// and removes by its id.
    fn learn_name(package: &std::path::Path) -> Option<(String, String)> {
        let (id, name) = crate::app_names::from_package(package)?;
        if let Err(err) = crate::app_names::remember(&id, &name) {
            tracing::warn!("could not remember the name of {id}: {err}");
        }
        Some((id, name))
    }

    /// Install an app over the link this supervisor already holds, and give
    /// its name, for the event log.
    ///
    /// The band takes one host, and that host is this process, so the install
    /// must reuse this session rather than open another. With New app on top
    /// on, the new app is then put first in the menu, which needs the chunked
    /// transport the install tore down, so that runs once the link is
    /// rebuilt: see [`Supervisor::installed`].
    async fn install_here(
        &mut self,
        file: &str,
        answer: &crate::ipc::Answer,
    ) -> Result<String, BandError> {
        let bytes = std::fs::read(file)
            .map_err(|err| BandError::Install(format!("cannot read {file}: {err}")))?;
        if bytes.len() as u64 > crate::install::MAX_APP_BYTES {
            return Err(BandError::FileTooLarge(bytes.len() as u64));
        }
        // The band will report this app as an id and nothing else, so read its
        // name now, while the package and its manifest are in front of us.
        let package = std::path::Path::new(file);
        let learned = Self::learn_name(package);
        let app = shown_app_name(package, learned.as_ref().map(|(_, name)| name.as_str()));
        self.newest_app = learned.map(|(id, _)| id);

        let Some(session) = self.session.as_ref() else {
            return Err(BandError::Install(
                "the band is not connected. Connect first.".to_string(),
            ));
        };
        answer.line(format!("{file}: {} bytes", bytes.len())).await;

        let peripheral = session.peripheral().clone();
        let read_char = session.chunked_read().clone();
        let fw = crate::ble::firmware_characteristics(&peripheral)?;
        crate::install::run(&peripheral, &fw, &read_char, &bytes, tell_progress(answer)).await?;

        Ok(app)
    }

    /// An install has ended, with the app's name or why it failed.
    ///
    /// The install unsubscribes the chunked transport, so a session it used
    /// is spent either way: it is dropped, and the loop brings the band back.
    /// With no link there was nothing to spend, and a connect or a retry that
    /// runs is left to run. An app that went on is logged, and whoever asked
    /// hears how it went.
    ///
    /// New app on top says whether the new app goes first (D10), as the file
    /// says it as the install ends. On, the new link puts the app first in the
    /// menu, and moves nothing else. Off, the menu stays as the band arranges
    /// it.
    async fn installed(
        &mut self,
        result: Result<String, BandError>,
        had_link: bool,
        answer: &crate::ipc::Answer,
    ) {
        self.follow_changes().await;
        let lift = result.is_ok() && self.settings.new_app_on_top;
        if !lift {
            // Nothing to lift: the switch was off, or nothing went on.
            self.newest_app = None;
        }
        if had_link {
            self.lift_when_ready = lift;
            self.stand(Happened::Reopening);
            self.teardown().await;
            self.attempt = 0;
        }
        match result {
            Ok(app) => {
                self.log_app(Change::Installed, app).await;
                answer.line("Installed.").await;
                answer.done().await;
            }
            Err(err) => answer.failed(err.to_string()).await,
        }
    }

    /// Tell the window's event log that an app was installed or removed.
    async fn log_app(&mut self, change: Change, app: String) {
        let record = AppChange {
            id: self.next_record(),
            at: SystemTime::now(),
            change,
            app,
        };
        self.send(Status::AppChanged(record)).await;
    }

    /// The number of the next record in the window's event log.
    fn next_record(&mut self) -> u64 {
        self.records += 1;
        self.records
    }

    /// List the apps on the band, or remove one. A removal is logged, by the
    /// app's name.
    ///
    /// Removing goes through the same link everything else uses, so it cannot
    /// run while the band is away.
    async fn serve_apps(
        &mut self,
        remove: Option<&str>,
        everything: bool,
    ) -> Result<Vec<String>, BandError> {
        let Some(session) = self.session.as_mut() else {
            return Err(BandError::Apps(
                "the band is not connected. Connect first.".to_string(),
            ));
        };
        let Some(id) = remove else {
            return crate::apps::list_lines(session, everything).await;
        };
        let app = crate::apps::remove(session, id).await?;
        Ok(self.removed(&app).await)
    }

    /// An app went off the band: log it, by its name or, with none known, its
    /// id, and give the line for whoever asked.
    async fn removed(&mut self, app: &crate::apps::Entry) -> Vec<String> {
        self.log_app(Change::Removed, app.name_or_id().to_string())
            .await;
        vec![crate::apps::removed_line(app)]
    }

    async fn serve_screens(
        &mut self,
        list: &str,
        set: Option<&[String]>,
        hide: &[String],
        tidy: bool,
    ) -> Result<Vec<String>, BandError> {
        let kind = crate::screens::kind_from_name(list)?;
        let Some(session) = self.session.as_mut() else {
            return Err(BandError::DisplayList(
                "the band is not connected. Connect first.".to_string(),
            ));
        };
        crate::screens::serve(session, kind, set, hide, tidy).await
    }

    /// Take one request from another process, if one is waiting.
    ///
    /// Written as its own future so the run loop can select on it without
    /// borrowing the whole supervisor.
    async fn next_request(
        requests: &mut Option<mpsc::Receiver<crate::ipc::Incoming>>,
    ) -> Option<crate::ipc::Incoming> {
        match requests {
            Some(rx) => rx.recv().await,
            // No socket: never ready, so the select falls to the other arms.
            None => std::future::pending().await,
        }
    }

    /// Run until a Shutdown command or a closed command channel.
    /// The link is always released before returning.
    pub async fn run(mut self) -> Result<(), BandError> {
        loop {
            // A change to the config file counts from the next thing that
            // happens, whatever woke the loop.
            self.follow_changes().await;
            // Whatever moved on the last pass is told before the next wait.
            self.report().await;

            // A connect in flight owns the state machine until it lands, and
            // this branch is what keeps requests answered while it runs.
            if self.connecting.is_some() {
                if !self.wait_for_connect().await {
                    self.quit().await;
                    return Ok(());
                }
                continue;
            }

            let action = next_action(self.desired(), &self.state, Instant::now());
            let keep_going = match action {
                Action::BeginConnect => {
                    self.begin_connect().await;
                    true
                }
                Action::Teardown => {
                    self.teardown().await;
                    true
                }
                Action::Wait(delay) => {
                    tokio::select! {
                        cmd = self.commands.recv() => self.apply(cmd).await,
                        incoming = Self::next_request(&mut self.requests) => {
                            self.serve_request(incoming).await
                        }
                        _ = tokio::time::sleep(delay) => true,
                    }
                }
                Action::Pump => self.pump().await,
                Action::Park => {
                    tokio::select! {
                        cmd = self.commands.recv() => self.apply(cmd).await,
                        incoming = Self::next_request(&mut self.requests) => {
                            self.serve_request(incoming).await
                        }
                    }
                }
            };

            if !keep_going {
                self.quit().await;
                return Ok(());
            }
        }
    }

    /// Let the band go before the loop ends. Quitting is a person letting it
    /// go, so a follower hears that last.
    async fn quit(&mut self) {
        self.stand(Happened::AskedToDisconnect);
        self.teardown().await;
        self.report().await;
    }

    /// Returns false when the loop should end.
    async fn apply(&mut self, command: Option<Command>) -> bool {
        match command {
            // A closed channel means the front end is gone. Shut down.
            None | Some(Command::Shutdown) => false,
            Some(Command::Connect) => {
                if let Err(reason) = self.ask_to_connect().await {
                    self.send(Status::Failed(reason)).await;
                }
                true
            }
            Some(Command::Disconnect) => {
                self.ask_to_disconnect().await;
                self.send(Status::Notice(
                    "disconnected by request; auto-reconnect suppressed".to_string(),
                ))
                .await;
                true
            }
            Some(Command::StopTrying) => {
                self.stop_trying().await;
                true
            }
            Some(Command::ReportState) => {
                // The front end cannot know the state, so answer from here,
                // even when nothing changed since it was last told.
                self.reported = None;
                self.report().await;
                true
            }
            Some(Command::ConfigChanged) => {
                self.follow_changes().await;
                true
            }
            Some(Command::Learn(learner)) => {
                self.learn(learner);
                true
            }
            Some(Command::StopLearning) => {
                self.stop_learning(Learned::Stopped);
                true
            }
        }
    }

    /// Take the next press for a learn: its signal goes back on `learner`,
    /// and nothing fires for it. A learn that waits already is stopped, so
    /// one learn runs at a time. With no band connected, no press can come,
    /// and the learn hears so at once.
    fn learn(&mut self, learner: oneshot::Sender<Learned>) {
        self.stop_learning(Learned::Stopped);
        if self.standing.state != Phase::Connected {
            let _ = learner.send(Learned::Failed(NOT_CONNECTED_TO_LEARN.to_string()));
            return;
        }
        self.learner = Some(learner);
    }

    /// End the learn that waits, if any, and tell it how.
    fn stop_learning(&mut self, how: Learned) {
        if let Some(learner) = self.learner.take() {
            let _ = learner.send(how);
        }
    }

    /// Whether a learn took this press. A learn whose window stopped waiting
    /// takes nothing, and the press fires as usual.
    fn learned(&mut self, signal: &str) -> bool {
        self.learner
            .take()
            .is_some_and(|learner| learner.send(Learned::Signal(signal.to_string())).is_ok())
    }

    /// A person asked for the link: Connect, Reconnect, or `connect` on the
    /// socket. Refused, with the reason, when there is nothing to connect to.
    ///
    /// With no key held and no pair running, the file and the store are asked
    /// again first: see [`Supervisor::look_again`].
    async fn ask_to_connect(&mut self) -> Result<(), String> {
        if self.auth_key.is_none() && !self.pairing() {
            self.look_again().await?;
        }
        self.stand(Happened::AskedToConnect);
        self.attempt = 0;
        // A pending pause ends, so a connect a person asked for starts now.
        self.end_backoff();
        self.report().await;
        Ok(())
    }

    /// A person asked to let the band go: Disconnect, or Cancel while
    /// connecting.
    ///
    /// A live link is left for the loop, which `next_action` sends to
    /// Teardown, and teardown() releases it. A connect in flight runs to its
    /// end, and lets the band go when it lands: stopping it part way would
    /// skip the cleanup that releases the band.
    async fn ask_to_disconnect(&mut self) {
        self.stand(Happened::AskedToDisconnect);
        self.attempt = 0;
        // A leftover pause would read as a retry that is not coming.
        self.end_backoff();
        self.report().await;
    }

    /// A person asked to stop the retries. The band stays away, for the
    /// reason it went and since the moment it went, as after a drop with auto
    /// reconnect off.
    ///
    /// Only a retry stops. A retry attempt in flight runs to its end, and lets
    /// the band go when it lands.
    async fn stop_trying(&mut self) {
        if self.standing.state != Phase::Retrying {
            return;
        }
        self.stop_retrying("stopped trying by request; the band stays disconnected")
            .await;
    }

    /// End the retries, and say why. The band stays away, for the reason it
    /// went and since the moment it went.
    async fn stop_retrying(&mut self, why: &str) {
        self.stand(Happened::AskedToStopTrying);
        self.attempt = 0;
        self.end_backoff();
        self.report().await;
        self.send(Status::Notice(why.to_string())).await;
    }

    /// End a pause between retries. Nothing is waiting out the pause now.
    fn end_backoff(&mut self) {
        if matches!(self.state, ConnState::Backoff { .. }) {
            self.state = ConnState::Idle;
        }
    }

    /// What the radio should do, from where the connection stands.
    fn desired(&self) -> Desired {
        self.standing.desired()
    }

    /// Move where the connection stands. The front ends hear it with the next
    /// report, which the loop makes before it waits again.
    ///
    /// A learn waits for a press, which only a connected band sends, so a
    /// band that goes away ends it.
    fn stand(&mut self, happened: Happened) {
        self.standing = self.standing.after(happened, SystemTime::now());
        if self.standing.state != Phase::Connected {
            self.stop_learning(Learned::Failed(WENT_BEFORE_A_PRESS.to_string()));
        }
    }

    /// Ask the band for its battery. The answer arrives on the message loop,
    /// like anything else the band sends.
    async fn ask_battery(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if let Err(err) = session
            .send(battery::ENDPOINT, &battery::request(), battery::ENCRYPTED)
            .await
        {
            tracing::debug!("battery request not sent: {err}");
        }
    }

    /// Start a connect in a task of its own, and return to the loop.
    async fn begin_connect(&mut self) {
        if self.connecting.is_some() {
            return;
        }
        let Some(auth_key) = self.auth_key.clone() else {
            // No band is paired. Nothing asks for a connect then, and if
            // something did, the answer is to wait for a pair.
            self.stand(Happened::AttemptFailed { retry: false });
            return;
        };
        self.set_state(ConnState::Scanning).await;
        let (progress, rx) = mpsc::channel(4);
        let opts = self.opts.clone();
        let status = self.status.clone();
        self.connecting = Some(Connecting {
            task: tokio::spawn(connect_once(opts, auth_key, status, progress)),
            progress: rx,
            purpose: Purpose::Connect,
        });
    }

    /// Take up the band the file names, and its key, for a Connect that found
    /// no key held. Refused, with the reason, when that cannot be done.
    ///
    /// The front end gives the supervisor the band and the key it could have
    /// at start, and that can be out of date: a file that did not load then
    /// may load now, and a store that could not be read then may answer now.
    /// So the file is read again, and the key loaded for the MAC it names,
    /// both on a blocking thread, as a pair and a forget use the store. Found,
    /// both are taken up, and the connect goes on. See [`band_and_key`] for
    /// the refusals.
    ///
    /// The loop waits here while the store answers, which may be after a
    /// dialog of the system's own. That is acceptable: with no key, no link
    /// is up, so no band traffic waits on the loop, and a connect could not
    /// begin before this answers anyway.
    async fn look_again(&mut self) -> Result<(), String> {
        let file = self.config_file.clone();
        let load = self.load_key;
        let (device, key) =
            tokio::task::spawn_blocking(move || band_and_key(file.as_deref(), load))
                .await
                .map_err(|err| {
                    format!("cannot connect: the look for the key stopped part way: {err}")
                })??;
        self.auth_key = Some(key);
        self.opts.mac = Some(device.mac);
        self.opts.peripheral_id = device.uuid;
        Ok(())
    }

    /// Whether a pair is running.
    fn pairing(&self) -> bool {
        matches!(
            self.connecting,
            Some(Connecting {
                purpose: Purpose::Pair(_),
                ..
            })
        )
    }

    /// Start a pair: check what the person gave, then look for the band that
    /// takes the key, in a task of its own, as a connect does. The loop goes
    /// on answering while it runs, and the answer goes back when it lands.
    async fn begin_pair(&mut self, key: KeyText, name: String, setup: Setup, answer: Answer) {
        let key = match self.may_pair(&key, &name) {
            Ok(key) => key,
            Err(err) => {
                answer.failed(err.to_string()).await;
                return;
            }
        };

        // A pair asks for the link, as Connect does. A Disconnect while it
        // runs is honored when it lands: the band is paired, and let go.
        self.stand(Happened::AskedToConnect);
        self.attempt = 0;
        self.set_state(ConnState::Scanning).await;
        let (progress, rx) = mpsc::channel(4);
        // The key decides which band is the one, so no band is expected.
        let opts = ConnectOptions {
            peripheral_id: None,
            mac: None,
            ..self.opts.clone()
        };
        let task = tokio::spawn(pair_once(
            opts,
            key.clone(),
            self.status.clone(),
            progress,
            answer.clone(),
        ));
        self.connecting = Some(Connecting {
            task,
            progress: rx,
            purpose: Purpose::Pair(PairJob {
                key,
                name: name.trim().to_string(),
                setup,
                answer,
            }),
        });
    }

    /// The key, when a pair may start with it and this name. Otherwise why
    /// not, in words fit to show. Nothing here reaches the band.
    fn may_pair(&self, key: &KeyText, name: &str) -> Result<AuthKey, BandError> {
        let key = AuthKey::from_hex(key.as_str())?;
        let name = name.trim();
        if name.is_empty() {
            return Err(BandError::Pairing(
                "the band needs a name, which the window shows".to_string(),
            ));
        }
        // Two fields side by side: the key can land in the wrong one. The
        // name is written to the file and shown, and the key must not be.
        if crate::config::holds_a_key(name) {
            return Err(BandError::Pairing(
                "the name looks like a key. Give the band a name to know it by, such as \
                 Xiaomi Smart Band 7."
                    .to_string(),
            ));
        }
        if self.pairing() {
            return Err(BandError::Pairing(
                "a pair is running already. Wait for it to finish.".to_string(),
            ));
        }
        if self.session.is_some() || self.connecting.is_some() {
            return Err(BandError::Pairing(
                "a band is connected, or connecting. Forget it before you pair another."
                    .to_string(),
            ));
        }
        if let Some(device) = Config::load()?.device {
            return Err(BandError::Pairing(format!(
                "{} is paired already. Forget it before you pair another band.",
                device.name
            )));
        }
        Ok(key)
    }

    /// Forget the paired band, and keep or delete its actions and settings.
    ///
    /// The key leaves the store first, and a store that keeps it leaves
    /// everything as it was. Then the band is let go, and last the file loses
    /// `[device]`. The lines say what was done.
    async fn forget(&mut self, setup: Setup) -> Result<Vec<String>, BandError> {
        if self.pairing() {
            return Err(BandError::Pairing(
                "a pair is running. Wait for it to finish, then forget.".to_string(),
            ));
        }
        let mut config = Config::load()?;
        // The store may wait on the person, in a dialog of the system's own.
        let forgotten = tokio::task::spawn_blocking(move || config.forget(setup, AuthKey::delete))
            .await
            .map_err(|err| BandError::KeyStore(format!("the forget stopped part way: {err}")))??;

        // Nothing is left to reach the band with, so let it go.
        self.auth_key = None;
        self.opts.peripheral_id = None;
        self.opts.mac = None;
        self.stand(Happened::AskedToDisconnect);
        self.attempt = 0;
        self.teardown().await;
        self.facts = crate::device::Facts::default();
        self.battery = None;

        // The file loses [device], and keeps or deletes the rest, as read now
        // and under the config lock: an action the window saved while the
        // band was let go is kept, or deleted with the others.
        let config = Config::update(|config| {
            config.device = None;
            config.keep_or_delete(setup);
            Ok::<_, BandError>(config.clone())
        })?;
        self.follow_the_file(&config);

        let kept = match setup {
            Setup::Keep => "The actions and settings are kept for the next band.",
            Setup::Delete => "The actions are deleted, and the settings are back to their defaults.",
        };
        Ok(match forgotten {
            Some(device) => vec![format!(
                "forgot {}. Its key is gone from {}. {kept}",
                device.name,
                crate::config::STORE
            )],
            None => vec![format!("no band was paired. {kept}")],
        })
    }

    /// Take up what the file now says about actions and settings, after this
    /// supervisor wrote it.
    fn follow_the_file(&mut self, config: &Config) {
        self.actions = config.actions.clone();
        self.settings = config.settings;
        self.config_seen = self.config_stamp();
    }

    /// Wait for the connect, while still answering everybody else.
    ///
    /// Returns false when the loop should end.
    async fn wait_for_connect(&mut self) -> bool {
        let Some(mut held) = self.connecting.take() else {
            return true;
        };
        tokio::select! {
            joined = &mut held.task => {
                match held.purpose {
                    Purpose::Connect => self.connect_landed(joined).await,
                    Purpose::Pair(job) => self.pair_landed(job, joined).await,
                }
                true
            }
            Some(state) = held.progress.recv() => {
                self.set_state(state).await;
                self.connecting = Some(held);
                true
            }
            cmd = self.commands.recv() => {
                // The task runs on. Stopping it part way through would skip the
                // cleanup that releases the band, and a band nobody released
                // cannot be reached by the phone either.
                self.connecting = Some(held);
                self.apply(cmd).await
            }
            incoming = Self::next_request(&mut self.requests) => {
                self.connecting = Some(held);
                self.serve_request(incoming).await
            }
        }
    }

    /// A connect to the paired band has ended, one way or the other.
    async fn connect_landed(
        &mut self,
        joined: Result<Result<Link, BandError>, tokio::task::JoinError>,
    ) {
        match joined {
            Ok(Ok(link)) => self.land(link).await,
            Ok(Err(err @ BandError::WrongBand { .. })) => {
                // Trying again would reach the same band, which is not this
                // person's to act on. They connect again when theirs is back.
                self.send(Status::Failed(err.to_string())).await;
                self.stand(Happened::AttemptFailed { retry: false });
                self.attempt = 0;
                self.set_state(ConnState::Idle).await;
            }
            Ok(Err(err)) => {
                self.send(Status::Failed(err.to_string())).await;
                self.attempt_failed().await;
            }
            Err(err) => {
                self.send(Status::Failed(format!("the connect task ended: {err}"))).await;
                self.attempt_failed().await;
            }
        }
    }

    /// Take a link that a connect or a pair opened, or let it go when nobody
    /// wants it now: a Cancel, a Stop trying or a forget came while it ran.
    ///
    /// A link let go here is never reported as connected, so the window
    /// never shows a link that is about to close.
    async fn land(&mut self, link: Link) {
        if self.desired() == Desired::Connected && self.auth_key.is_some() {
            return self.adopt(link).await;
        }
        if let Err(err) = link.session.disconnect().await {
            tracing::warn!("could not release the band: {err}");
        }
        self.attempt = 0;
        self.set_state(ConnState::Idle).await;
    }

    /// An attempt to connect failed. Try again after a pause, unless a person
    /// let the band go while the attempt ran.
    async fn attempt_failed(&mut self) {
        self.stand(Happened::AttemptFailed { retry: true });
        if self.desired() == Desired::Disconnected {
            self.attempt = 0;
            self.set_state(ConnState::Idle).await;
        } else {
            self.fail_and_back_off().await;
        }
    }

    /// Take a new link as this supervisor's own.
    async fn adopt(&mut self, link: Link) {
        self.session = Some(link.session);
        self.facts = link.facts;
        self.attempt = 0;
        self.stand(Happened::LinkUp);
        self.set_state(ConnState::Ready).await;
        self.set_band_clock().await;
        // Ask once here, so a window that opens on a connected band has a
        // level to show without asking for one.
        self.ask_battery().await;
        // New app on top as the file says it now, not as it was at start.
        self.follow_changes().await;
        self.lift_if_owed().await;
    }

    /// A pair has ended. On success the band's key is kept, `[device]` is
    /// written, and the link the pair opened becomes the connection, so the
    /// band is connected without a second link. A Disconnect that came while
    /// it ran lets that link go instead. Then the person hears back.
    async fn pair_landed(
        &mut self,
        job: PairJob,
        joined: Result<Result<Link, BandError>, tokio::task::JoinError>,
    ) {
        let link = match joined {
            Ok(Ok(link)) => link,
            Ok(Err(err)) => return self.pair_failed(job.answer, err.to_string()).await,
            Err(err) => {
                return self
                    .pair_failed(job.answer, format!("the pair stopped part way: {err}"))
                    .await;
            }
        };
        let device = match self.keep_pair(&job, &link).await {
            Ok(device) => device,
            Err(err) => {
                if let Err(release_err) = link.session.disconnect().await {
                    tracing::warn!("could not release the band: {release_err}");
                }
                return self.pair_failed(job.answer, err.to_string()).await;
            }
        };

        self.auth_key = Some(job.key);
        self.opts.peripheral_id = device.uuid.clone();
        self.opts.mac = Some(device.mac.clone());
        self.land(link).await;
        let paired = format!("paired with {}, {}", device.name, device.mac);
        self.send(Status::Notice(paired.clone())).await;
        job.answer.line(paired).await;
        job.answer.done().await;
    }

    /// Keep what a pair found: the key in the store, under the MAC the band
    /// read out, then `[device]` in the file, with the actions and settings
    /// kept or deleted as the person chose.
    ///
    /// The key goes first, so the file never names a band whose key is not
    /// kept. A file that cannot be written afterwards leaves the key in the
    /// store, under that band's MAC, which is where it belongs.
    async fn keep_pair(&mut self, job: &PairJob, link: &Link) -> Result<Device, BandError> {
        let Some(mac) = link.facts.mac.clone() else {
            return Err(BandError::Pairing(
                "the band took the key, but did not say its address, so Mi Band Bridge cannot \
                 keep it. Try again."
                    .to_string(),
            ));
        };
        // Read again: the file may have changed while the search ran.
        if let Some(device) = Config::load()?.device {
            return Err(already_paired(&device));
        }

        // The store may wait on the person, in a dialog of the system's own.
        let (key, account) = (job.key.clone(), mac.clone());
        tokio::task::spawn_blocking(move || key.keep_for(&account))
            .await
            .map_err(|err| BandError::KeyStore(format!("the key was not kept: {err}")))??;

        // Read, changed and written under the config lock, so an action or a
        // setting the window saved while the key was kept stays.
        let device = Device::found(&job.name, &mac, &link.id);
        let config = Config::update(|config| {
            if let Some(paired) = &config.device {
                return Err(already_paired(paired));
            }
            config.device = Some(device.clone());
            config.keep_or_delete(job.setup);
            Ok(config.clone())
        })?;
        self.follow_the_file(&config);
        Ok(device)
    }

    /// Say why a pair failed, to whoever asked and to the front ends, and go
    /// back to waiting.
    async fn pair_failed(&mut self, answer: Answer, reason: String) {
        // With no key, nothing is left to connect to. A key from the
        // environment still has its band to reach.
        self.stand(Happened::AttemptFailed {
            retry: self.auth_key.is_some(),
        });
        self.attempt = 0;
        self.set_state(ConnState::Idle).await;
        self.send(Status::Failed(reason.clone())).await;
        answer.failed(reason).await;
    }

    /// Read one message from a live session, or take a command.
    /// Returns false when the loop should end.
    async fn pump(&mut self) -> bool {
        if self.session.is_none() {
            // Ready with no session should not happen. Recover rather than panic.
            self.set_state(ConnState::Idle).await;
            return true;
        }

        // `session` and `commands` are separate fields, so destructure to borrow
        // them disjointly. Calling self.commands.recv() while self.session is
        // mutably borrowed would not compile.
        let next = {
            let Self {
                session,
                commands,
                requests,
                ..
            } = self;
            let session = session.as_mut().expect("checked immediately above");
            tokio::select! {
                cmd = commands.recv() => Next::Command(cmd),
                // A connected band is exactly when another process wants it, so
                // requests are taken here too. Waiting only in the idle arms
                // would leave a caller hanging for as long as the link is up.
                incoming = Self::next_request(requests) => Next::Request(incoming),
                result = session.next_message(READ_TIMEOUT) => Next::Message(result),
            }
        };

        let outcome = match next {
            Next::Command(cmd) => return self.apply(cmd).await,
            Next::Request(incoming) => return self.serve_request(incoming).await,
            Next::Message(result) => result,
        };

        match outcome {
            Ok(message) => self.heard(message).await,
            // A quiet band is not a dropped link.
            Err(BandError::Timeout) => tracing::debug!("idle, still connected"),
            Err(err) => self.link_lost(err).await,
        }
        true
    }

    /// One message from the band: remember a battery report, fire the action
    /// a press names, and tell the front ends.
    async fn heard(&mut self, message: crate::protocol::chunk::ZeppMessage) {
        let at = SystemTime::now();
        let event = BandEvent::from_message(message);

        // Remember the level whether we asked for it or the band volunteered
        // it. This is before the filter below, which keeps a battery report
        // out of the way.
        if let BandEvent::Battery(state) = &event {
            self.battery = Some((*state, Instant::now()));
        }

        // The band's own service traffic is not a press. Keep it for --raw,
        // and out of the way otherwise.
        if !event.is_interesting() && !self.opts.raw {
            tracing::debug!("band housekeeping: {event}");
            return;
        }
        // An edit to the file takes effect here, with no reconnect: an action
        // is a lookup on this side and touches nothing on the band.
        self.follow_changes().await;

        match event.signal() {
            Some(signal) => self.pressed(event, signal, at).await,
            // Not a press: a terminal shows it as it came, and the window's
            // event log leaves it out.
            None => self.send(Status::Event(event)).await,
        }
    }

    /// A press came, at `at`. Start the action its signal names, if any, then
    /// tell the front ends: the band's bytes, then the press, for the event
    /// log.
    ///
    /// Firing here, rather than in a front end, is what makes a press do what
    /// it says whichever front end is running. The action starts before
    /// anyone is told, so no front end can hold it up. A keystroke is over by
    /// then, and is told with its result. Anything slower is told now, with
    /// none, and again, with its result, when it ends, from a task that is
    /// spawned only once the press is told, so nobody hears a result before
    /// its press.
    ///
    /// A learn takes the press first, if one waits: the window's editor gets
    /// the signal, and nothing fires. That press is the answer to the
    /// window's own request, so it is no line of the event log either. A
    /// terminal still shows the band's bytes, and says what became of them.
    async fn pressed(&mut self, event: BandEvent, signal: String, at: SystemTime) {
        if self.learned(&signal) {
            self.send(Status::Event(event)).await;
            self.send(Status::Notice(format!(
                "the window learned {signal}, so nothing fired"
            )))
            .await;
            return;
        }
        let mut queued = crate::binding::matching(&self.actions, &signal)
            .into_iter()
            .map(|action| Queued {
                name: action.name.clone(),
                shown: crate::binding::describe(&action.does),
                does: action.does.clone(),
            })
            .collect::<Vec<_>>()
            .into_iter();
        // The first starts before anyone is told, so no front end can hold it
        // up, and a keystroke is over by the time the press is told.
        let fired = queued.next().map(|first| Fired {
            started: crate::action::start(&first.does),
            name: first.name,
            does: first.shown,
        });
        self.tell_press(event, signal, at, fired, queued.collect())
            .await;
    }

    /// Tell the front ends about a press and the actions it fires, one record
    /// each, in the order they run, and about each action's result once it
    /// ends: never before the press.
    ///
    /// Several actions may answer one signal (D6), and they run one by one:
    /// each starts when the one before has ended. `fired` is the first, which
    /// has started already. The `rest` run in a task of their own, spawned
    /// only once the press is told, so a slow action never holds up the band,
    /// and nobody hears a result before its press.
    async fn tell_press(
        &mut self,
        event: BandEvent,
        signal: String,
        at: SystemTime,
        fired: Option<Fired>,
        rest: Vec<Queued>,
    ) {
        let blank = |id| Press {
            id,
            at,
            signal: signal.clone(),
            action: None,
            does: None,
            result: None,
        };
        let Some(Fired {
            started,
            name,
            does,
        }) = fired
        else {
            let press = blank(self.next_record());
            self.send(Status::Event(event)).await;
            self.send(Status::Pressed(press)).await;
            return;
        };

        let mut first = Press {
            action: Some(name),
            does: Some(does),
            ..blank(self.next_record())
        };
        let mut running = None;
        match started {
            Started::Finished(outcome) => first.result = Some(outcome),
            Started::Running(task) => running = Some(task),
        }
        let mut waiting = Vec::new();
        for action in rest {
            let press = Press {
                action: Some(action.name),
                does: Some(action.shown),
                ..blank(self.next_record())
            };
            waiting.push((press, action.does));
        }

        self.send(Status::Event(event)).await;
        self.send(Status::Pressed(first.clone())).await;
        for (press, _) in &waiting {
            self.send(Status::Pressed(press.clone())).await;
        }

        if running.is_none() && waiting.is_empty() {
            return;
        }
        // Straight to the front ends' channel, as an action's own report
        // always went: `follow` shows the press, and not its outcome.
        let status = self.status.clone();
        tokio::spawn(async move {
            if let Some(task) = running {
                let outcome = crate::action::finished(task).await;
                let finished = Press {
                    result: Some(outcome),
                    ..first
                };
                let _ = status.send(Status::Finished(finished)).await;
            }
            for (press, does) in waiting {
                let outcome = match crate::action::start(&does) {
                    Started::Finished(outcome) => outcome,
                    Started::Running(task) => crate::action::finished(task).await,
                };
                let finished = Press {
                    result: Some(outcome),
                    ..press
                };
                let _ = status.send(Status::Finished(finished)).await;
            }
        });
    }

    /// The link went down by itself. Try again, or stay down, as auto
    /// reconnect says now: a change made while the link was up counts.
    async fn link_lost(&mut self, err: BandError) {
        self.send(Status::Failed(format!("link lost: {err}"))).await;
        self.session = None;
        self.follow_changes().await;
        // Only this path consults auto reconnect, and it can only retry a
        // link that was wanted, never revive a disconnect.
        self.stand(Happened::LinkLost {
            retry: self.auto_reconnect(),
        });
        if self.desired() == Desired::Disconnected {
            // Auto reconnect is off. Report the truth: we are done, not retrying.
            self.attempt = 0;
            self.set_state(ConnState::Idle).await;
        } else {
            self.fail_and_back_off().await;
        }
    }

    async fn fail_and_back_off(&mut self) {
        self.attempt = self.attempt.saturating_add(1);
        let delay = backoff_delay(self.attempt);
        let state = ConnState::Backoff {
            until: Instant::now() + delay,
            attempt: self.attempt,
        };
        self.set_state(state).await;
    }

    async fn teardown(&mut self) {
        if let Some(session) = self.session.take()
            && let Err(err) = session.disconnect().await
        {
            tracing::warn!("disconnect failed: {err}");
        }
        if self.state != ConnState::Idle {
            self.set_state(ConnState::Idle).await;
        }
    }

    async fn set_state(&mut self, state: ConnState) {
        self.state = state;
        self.report().await;
    }

    /// Tell the front ends what the radio is doing and where the connection
    /// stands, when either changed since they were last told.
    async fn report(&mut self) {
        let now = (self.state.clone(), self.standing);
        if self.reported.as_ref() == Some(&now) {
            return;
        }
        self.reported = Some(now.clone());
        let (state, standing) = now;
        self.send(Status::State(state, standing)).await;
    }

    async fn send(&mut self, status: Status) {
        // A closed status channel is not fatal; the loop ends on the next command read.
        // Anyone following from another process sees the words `listen`
        // prints, from the same function, so the two never disagree.
        if !self.followers.is_empty()
            && let Some(line) = status.terminal_line()
        {
            let mut alive = Vec::new();
            for follower in std::mem::take(&mut self.followers) {
                if follower.0.send(crate::ipc::Reply::Line { text: line.clone() }).await.is_ok() {
                    alive.push(follower);
                }
            }
            self.followers = alive;
        }
        let _ = self.status.send(status).await;
    }
}

/// Run the Zepp 2021 handshake on an open session.
pub async fn authenticate(session: &mut Session, auth_key: AuthKey) -> Result<(), BandError> {
    let mut auth = Authenticator::new(auth_key, KeyPair::generate()?);
    session
        .send(ENDPOINT_AUTH, &auth.public_key_message(), false)
        .await?;

    loop {
        let message = session.next_message(AUTH_TIMEOUT).await?;
        if message.endpoint != ENDPOINT_AUTH {
            tracing::debug!(
                "ignoring endpoint {:#06x} during the handshake",
                message.endpoint
            );
            continue;
        }

        match auth.handle(&message.payload)? {
            AuthStep::SendChallenge { payload, params } => {
                // Both sides must know the key before the challenge goes out.
                session.set_session_params(params.sequence, params.session_key);
                session.send(ENDPOINT_AUTH, &payload, false).await?;
            }
            AuthStep::Authenticated => return Ok(()),
            AuthStep::Ignored => tracing::debug!("unhandled auth payload"),
        }
    }
}

/// No test here sends a pair or a forget past the checks that come before the
/// config file: past them, both read and write the live config, and a forget
/// deletes a key from the system's secret store. The config module tests
/// forgetting with a stand-in store and a test account instead.
#[cfg(test)]
mod supervisor_tests {
    use super::*;
    use crate::action::Outcome;
    use crate::config::{Appearance, AuthKey, SettingsChange};
    use crate::ipc::{Incoming, Reply, Request};
    use tokio::sync::mpsc;

    /// A key for the tests. It opens no band.
    const FAKE_KEY: &str = "000102030405060708090a0b0c0d0e0f";

    fn test_supervisor() -> (Supervisor, mpsc::Sender<Command>, mpsc::Receiver<Status>) {
        supervisor_with(Some(AuthKey::from_hex(FAKE_KEY).unwrap()))
    }

    fn test_options() -> ConnectOptions {
        ConnectOptions {
            peripheral_id: None,
            mac: None,
            raw: false,
            mtu: None,
            subscribe_all: false,
            scan_seconds: 1,
        }
    }

    /// A supervisor that follows no config file: none of these tests may
    /// read the live one. The tests of settings give it a file of its own.
    /// It reaches no key store either: see [`no_keychain`].
    fn supervisor_with(
        auth_key: Option<AuthKey>,
    ) -> (Supervisor, mpsc::Sender<Command>, mpsc::Receiver<Status>) {
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(8);
        let (status_tx, status_rx) = mpsc::channel::<Status>(64);
        let mut supervisor = Supervisor::new(auth_key, test_options(), cmd_rx, status_tx);
        supervisor.config_file = None;
        supervisor.load_key = no_keychain;
        (supervisor, cmd_tx, status_rx)
    }

    /// The key loader every test supervisor starts with, in place of
    /// [`AuthKey::load`]. It reaches no store: a test that means to find a
    /// key gives a loader of its own.
    fn no_keychain(_: Option<&str>) -> Result<AuthKey, BandError> {
        panic!("a test asked for a key, and gave no loader of its own")
    }

    /// Everything the supervisor answers to one request.
    async fn ask(sup: &mut Supervisor, request: Request) -> Vec<Reply> {
        let (tx, mut rx) = mpsc::channel(32);
        assert!(sup.serve_request(Some(Incoming { request, reply: tx })).await);
        let mut replies = Vec::new();
        while let Ok(reply) = rx.try_recv() {
            replies.push(reply);
        }
        replies
    }

    fn failed(replies: &[Reply]) -> String {
        match replies.last() {
            Some(Reply::Failed { reason }) => reason.clone(),
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    fn pair_request(key: &str, name: &str) -> Request {
        Request::Pair {
            key: KeyText::new(key),
            name: name.to_string(),
            setup: Setup::Keep,
        }
    }

    /// With no band paired, the supervisor does no radio work at all.
    #[tokio::test]
    async fn with_no_band_paired_it_waits() {
        let (sup, _cmd_tx, _status_rx) = supervisor_with(None);
        assert_eq!(sup.desired(), Desired::Disconnected);
        assert_eq!(next_action(sup.desired(), &sup.state, Instant::now()), Action::Park);
        assert_eq!(sup.standing.state, Phase::Disconnected);
        assert_eq!(sup.standing.reason, Reason::Started);
    }

    #[tokio::test]
    async fn connect_with_no_band_paired_says_so_and_stays_put() {
        let (mut sup, _cmd_tx, mut status_rx) = supervisor_with(None);
        let before = sup.standing;
        assert!(sup.apply(Some(Command::Connect)).await);
        assert_eq!(sup.desired(), Desired::Disconnected);
        match status_rx.try_recv() {
            Ok(Status::Failed(reason)) => assert_eq!(reason, NOTHING_TO_CONNECT),
            other => panic!("expected the reason, got {other:?}"),
        }

        let replies = ask(&mut sup, Request::Connect).await;
        assert_eq!(failed(&replies), NOTHING_TO_CONNECT);
        assert_eq!(sup.desired(), Desired::Disconnected);
        assert_eq!(sup.standing, before, "a refused connect moved the standing");
    }

    // --- a connect with no key held looks again --------------------------------

    /// The band of these tests. It is made up, and each test gives a key
    /// loader of its own in place of the store, so no test reaches the
    /// Keychain.
    const TEST_MAC: &str = "0A:1B:2C:3D:4E:5F";
    const TEST_UUID: &str = "11111111-2222-4333-8444-555555555555";

    /// A file that names the band, as a pair writes it.
    const PAIRED: &str = "[device]\nname = \"Band\"\nmac = \"0A:1B:2C:3D:4E:5F\"\n\
                          uuid = \"11111111-2222-4333-8444-555555555555\"\n";

    /// A file that does not load.
    const BROKEN: &str = "[settings]\nauto_reconnect = \"yes\"\n";

    /// A supervisor that holds no key, as the window starts one when the file
    /// did not load, or the store gave no key. It follows a file of its own,
    /// which starts as `text`, and gets a key from `load_key`.
    fn keyless(
        text: &str,
        load_key: KeyLoader,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        Supervisor,
        mpsc::Receiver<Status>,
    ) {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("config.toml");
        std::fs::write(&path, text).unwrap();
        let (mut sup, _cmd_tx, status_rx) = supervisor_with(None);
        sup.config_file = Some(path.clone());
        sup.load_key = load_key;
        (folder, path, sup, status_rx)
    }

    /// The window's Connect, and why it was refused, or None when it was not.
    async fn window_connect(
        sup: &mut Supervisor,
        status_rx: &mut mpsc::Receiver<Status>,
    ) -> Option<String> {
        assert!(sup.apply(Some(Command::Connect)).await);
        let mut refused = None;
        while let Ok(status) = status_rx.try_recv() {
            if let Status::Failed(reason) = status {
                refused = Some(reason);
            }
        }
        refused
    }

    /// The window started while the file did not load, so it gave the band
    /// task no band and no key. Once the file is fixed, a Connect takes up the
    /// band the file names, and the key the store gives for its MAC, and
    /// connects, with no restart.
    #[tokio::test]
    async fn a_connect_after_the_file_is_fixed_takes_up_the_band_and_its_key() {
        let (_folder, path, mut sup, mut status_rx) = keyless(BROKEN, |mac| {
            assert_eq!(
                mac,
                Some(TEST_MAC),
                "the key is looked up by the file's MAC"
            );
            Ok(AuthKey::from_hex(FAKE_KEY).unwrap())
        });
        let refused = window_connect(&mut sup, &mut status_rx).await;
        assert!(refused.is_some(), "a file that does not load connected");

        std::fs::write(&path, PAIRED).unwrap();
        assert_eq!(window_connect(&mut sup, &mut status_rx).await, None);
        assert_eq!(sup.desired(), Desired::Connected);
        assert_eq!(sup.standing.state, Phase::Connecting);
        assert_eq!(sup.standing.reason, Reason::You);
        assert_eq!(
            next_action(sup.desired(), &sup.state, Instant::now()),
            Action::BeginConnect
        );
        let key = sup.auth_key.as_ref().expect("the key is taken up");
        assert_eq!(key.bytes(), AuthKey::from_hex(FAKE_KEY).unwrap().bytes());
        assert_eq!(sup.opts.mac.as_deref(), Some(TEST_MAC));
        assert_eq!(sup.opts.peripheral_id.as_deref(), Some(TEST_UUID));
    }

    /// A store that could not be read says why, on the socket and in the
    /// window alike. It never says to forget: the key may well be in the
    /// store, and a forget would delete it.
    #[tokio::test]
    async fn a_store_that_cannot_be_read_says_why_and_never_to_forget() {
        let (_folder, _path, mut sup, mut status_rx) = keyless(PAIRED, |_| {
            Err(BandError::KeyStore(
                "the Keychain cannot read the key for 0A:1B:2C:3D:4E:5F: User interaction \
                 is not allowed."
                    .to_string(),
            ))
        });
        let before = sup.standing;
        let said = "cannot connect: the Keychain cannot read the key for 0A:1B:2C:3D:4E:5F: \
                    User interaction is not allowed.";
        assert_eq!(failed(&ask(&mut sup, Request::Connect).await), said);
        assert_eq!(
            window_connect(&mut sup, &mut status_rx).await.as_deref(),
            Some(said)
        );
        assert!(sup.auth_key.is_none());
        assert_eq!(sup.opts.mac, None, "a refused connect took up the band");
        assert_eq!(sup.desired(), Desired::Disconnected);
        assert_eq!(sup.standing, before, "a refused connect moved the standing");
    }

    /// A store that holds no key for the band's MAC: forgetting the band and
    /// pairing it again is the way out, and safe, since the store holds no
    /// key to lose.
    #[tokio::test]
    async fn a_store_with_no_key_says_to_forget_and_pair_again() {
        let (_folder, _path, mut sup, mut status_rx) = keyless(PAIRED, |_| {
            Err(BandError::MissingAuthKey(
                "BAND_AUTH_KEY is not set, and the Keychain holds no key for 0A:1B:2C:3D:4E:5F"
                    .to_string(),
            ))
        });
        assert_eq!(
            failed(&ask(&mut sup, Request::Connect).await),
            NO_KEY_TO_CONNECT
        );
        assert_eq!(
            window_connect(&mut sup, &mut status_rx).await.as_deref(),
            Some(NO_KEY_TO_CONNECT)
        );
        assert!(sup.auth_key.is_none());
        assert_eq!(sup.desired(), Desired::Disconnected);
    }

    /// A file that names no band: there is nothing to connect to, and no
    /// store is asked.
    #[tokio::test]
    async fn a_file_with_no_band_says_so_and_asks_no_store() {
        let (_folder, _path, mut sup, mut status_rx) =
            keyless("[settings]\nauto_reconnect = true\n", |_| {
                panic!("the store was asked, with no band in the file")
            });
        assert_eq!(
            failed(&ask(&mut sup, Request::Connect).await),
            NOTHING_TO_CONNECT
        );
        assert_eq!(
            window_connect(&mut sup, &mut status_rx).await.as_deref(),
            Some(NOTHING_TO_CONNECT)
        );
        assert_eq!(sup.desired(), Desired::Disconnected);
    }

    /// A file that does not load gives its own reason, and does not say that
    /// no band is paired: one may well be. No store is asked.
    #[tokio::test]
    async fn a_file_that_does_not_load_gives_its_reason() {
        let (_folder, _path, mut sup, mut status_rx) = keyless(BROKEN, |_| {
            panic!("the store was asked, for a file that did not load")
        });
        let reason = failed(&ask(&mut sup, Request::Connect).await);
        assert!(
            reason.starts_with(
                "the config file could not be read, so Mi Band Bridge cannot connect until it \
                 is fixed: line 2: invalid type: string \"yes\", expected a boolean"
            ),
            "{reason}"
        );
        assert!(!reason.contains("no band is paired"), "{reason}");
        assert_eq!(window_connect(&mut sup, &mut status_rx).await, Some(reason));
        assert!(sup.auth_key.is_none());
        assert_eq!(sup.desired(), Desired::Disconnected);
    }

    /// The first named error: a key that is not 32 hex digits. It is refused
    /// before anything reaches the band, and no reply holds it.
    #[tokio::test]
    async fn a_pair_with_a_key_that_is_not_32_hex_digits_is_refused() {
        let (mut sup, _cmd_tx, _status_rx) = supervisor_with(None);
        for key in [&FAKE_KEY[..30], "0x0001020304050607080g0a0b0c0d0e0f", ""] {
            let replies = ask(&mut sup, pair_request(key, "Band")).await;
            let reason = failed(&replies);
            assert!(reason.starts_with("invalid auth key: a key is 32 hex digits"), "{reason}");
            assert!(!format!("{replies:?}").contains(&FAKE_KEY[..16]), "{replies:?}");
            assert!(sup.connecting.is_none(), "a bad key started a search");
            assert_eq!(sup.state, ConnState::Idle);
        }
    }

    /// A name is needed, and a key pasted into the name is refused, because
    /// the name is written to the file and shown.
    #[tokio::test]
    async fn a_pair_needs_a_name_that_is_not_a_key() {
        let (mut sup, _cmd_tx, _status_rx) = supervisor_with(None);
        let reason = failed(&ask(&mut sup, pair_request(FAKE_KEY, "  ")).await);
        assert_eq!(reason, "the band needs a name, which the window shows");

        let replies = ask(&mut sup, pair_request(FAKE_KEY, &format!("0x{FAKE_KEY}"))).await;
        assert!(failed(&replies).starts_with("the name looks like a key"), "{replies:?}");
        assert!(!format!("{replies:?}").contains(&FAKE_KEY[..16]), "{replies:?}");
        assert!(sup.connecting.is_none());
    }

    /// One pair at a time, and no forget while one runs.
    #[tokio::test]
    async fn a_running_pair_holds_off_another_pair_and_a_forget() {
        let (mut sup, _cmd_tx, _status_rx) = supervisor_with(None);
        let (progress_tx, progress) = mpsc::channel(1);
        drop(progress_tx);
        sup.connecting = Some(Connecting {
            task: tokio::spawn(async { Err(BandError::Timeout) }),
            progress,
            purpose: Purpose::Pair(PairJob {
                key: AuthKey::from_hex(FAKE_KEY).unwrap(),
                name: "Band".to_string(),
                setup: Setup::Keep,
                answer: Answer(mpsc::channel(1).0),
            }),
        });

        let reason = failed(&ask(&mut sup, pair_request(FAKE_KEY, "Band")).await);
        assert_eq!(reason, "a pair is running already. Wait for it to finish.");
        let reason = failed(&ask(&mut sup, Request::Forget { setup: Setup::Keep }).await);
        assert_eq!(reason, "a pair is running. Wait for it to finish, then forget.");

        // Connect while a pair runs is the pair's to answer, not a refusal.
        let replies = ask(&mut sup, Request::Connect).await;
        assert!(matches!(replies.last(), Some(Reply::Done)), "{replies:?}");
    }

    /// The window writes the clipboard itself. The band task says it cannot,
    /// and hands out nothing.
    #[tokio::test]
    async fn only_the_window_copies_the_key() {
        let (mut sup, _cmd_tx, _status_rx) = test_supervisor();
        let replies = ask(&mut sup, Request::CopyKey).await;
        assert_eq!(replies.len(), 1, "{replies:?}");
        assert!(failed(&replies).starts_with("only the Mi Band Bridge window"));
    }

    #[tokio::test]
    async fn disconnect_clears_a_pending_backoff() {
        let (mut sup, _cmd_tx, _status_rx) = test_supervisor();
        sup.standing = standing(Phase::Retrying, Reason::Dropped, ago(40));
        sup.state = ConnState::Backoff {
            until: Instant::now() + Duration::from_secs(30),
            attempt: 3,
        };

        assert!(sup.apply(Some(Command::Disconnect)).await);

        assert_eq!(sup.desired(), Desired::Disconnected);
        assert_eq!(
            sup.state,
            ConnState::Idle,
            "status must read `disconnected`, not a stale backoff"
        );
        assert_eq!(sup.standing.state, Phase::Disconnected);
        assert_eq!(sup.standing.reason, Reason::You);
    }

    #[tokio::test]
    async fn disconnect_leaves_a_live_session_for_teardown() {
        // Ready is torn down by next_action -> Action::Teardown, not by apply.
        let (mut sup, _cmd_tx, _status_rx) = test_supervisor();
        sup.standing = standing(Phase::Connected, Reason::Started, ago(60));
        sup.state = ConnState::Ready;

        assert!(sup.apply(Some(Command::Disconnect)).await);

        assert_eq!(sup.desired(), Desired::Disconnected);
        assert_eq!(sup.state, ConnState::Ready, "teardown owns the Ready case");
        assert_eq!(
            next_action(sup.desired(), &sup.state, Instant::now()),
            Action::Teardown
        );
        // The window shows the disconnect at once, before the link is down.
        assert_eq!(sup.standing.state, Phase::Disconnected);
        assert_eq!(sup.standing.reason, Reason::You);
    }

    #[tokio::test]
    async fn connect_still_clears_a_pending_backoff() {
        let (mut sup, _cmd_tx, _status_rx) = test_supervisor();
        sup.standing = standing(Phase::Retrying, Reason::Dropped, ago(40));
        sup.state = ConnState::Backoff {
            until: Instant::now() + Duration::from_secs(30),
            attempt: 4,
        };

        assert!(sup.apply(Some(Command::Connect)).await);

        assert_eq!(sup.desired(), Desired::Connected);
        assert_eq!(sup.state, ConnState::Idle);
        assert_eq!(sup.standing.state, Phase::Connecting, "a connect you asked for, at once");
        assert_eq!(sup.standing.reason, Reason::You);
        assert_eq!(
            next_action(sup.desired(), &sup.state, Instant::now()),
            Action::BeginConnect
        );
    }

    /// The socket's connect and disconnect do what the window's buttons do,
    /// pending pause and all.
    #[tokio::test]
    async fn the_socket_connects_and_disconnects_as_the_window_does() {
        let (mut sup, _cmd_tx, _status_rx) = test_supervisor();
        let pause = || ConnState::Backoff {
            until: Instant::now() + Duration::from_secs(30),
            attempt: 2,
        };

        sup.standing = standing(Phase::Retrying, Reason::Dropped, ago(40));
        sup.state = pause();
        let replies = ask(&mut sup, Request::Disconnect).await;
        assert_eq!(replies.last(), Some(&Reply::Done));
        assert_eq!(sup.state, ConnState::Idle, "status must not read a stale backoff");
        assert_eq!(sup.standing.state, Phase::Disconnected);
        assert_eq!(sup.standing.reason, Reason::You);

        sup.standing = standing(Phase::Retrying, Reason::Dropped, ago(40));
        sup.state = pause();
        let replies = ask(&mut sup, Request::Connect).await;
        assert_eq!(replies.last(), Some(&Reply::Done));
        assert_eq!(sup.state, ConnState::Idle);
        assert_eq!(sup.standing.state, Phase::Connecting);
        assert_eq!(sup.standing.reason, Reason::You);
    }

    // --- stop trying and cancel ---------------------------------------------

    /// Stop trying after a drop: the retries end, and the band stays away,
    /// since the drop. The front ends hear it once.
    #[tokio::test]
    async fn stop_trying_ends_the_retries_after_a_drop() {
        let (mut sup, _cmd_tx, mut status_rx) = test_supervisor();
        let dropped = standing(Phase::Retrying, Reason::Dropped, ago(40));
        sup.standing = dropped;
        sup.attempt = 3;
        sup.state = ConnState::Backoff {
            until: Instant::now() + Duration::from_secs(30),
            attempt: 3,
        };

        assert!(sup.apply(Some(Command::StopTrying)).await);

        let stopped = standing(Phase::Disconnected, Reason::Dropped, dropped.since);
        assert_eq!(sup.standing, stopped);
        assert_eq!(sup.state, ConnState::Idle, "a pause for a retry that is not coming");
        assert_eq!(sup.attempt, 0);
        assert_eq!(next_action(sup.desired(), &sup.state, Instant::now()), Action::Park);

        match status_rx.try_recv() {
            Ok(Status::State(state, told)) => {
                assert_eq!(state, ConnState::Idle);
                assert_eq!(told, stopped);
            }
            other => panic!("expected the new state, got {other:?}"),
        }
        assert!(matches!(status_rx.try_recv(), Ok(Status::Notice(_))));
        assert!(status_rx.try_recv().is_err(), "told more than once");
    }

    /// A band that came back a moment before the press stays connected.
    #[tokio::test]
    async fn stop_trying_leaves_a_band_that_came_back_connected() {
        let (mut sup, _cmd_tx, mut status_rx) = test_supervisor();
        let back = standing(Phase::Connected, Reason::Dropped, ago(1));
        sup.standing = back;
        sup.state = ConnState::Ready;
        sup.reported = Some((ConnState::Ready, back));

        assert!(sup.apply(Some(Command::StopTrying)).await);

        assert_eq!(sup.standing, back);
        assert_eq!(sup.state, ConnState::Ready);
        assert_eq!(next_action(sup.desired(), &sup.state, Instant::now()), Action::Pump);
        assert!(status_rx.try_recv().is_err(), "nothing changed, and something was told");
    }

    /// A connect in flight, as `begin_connect` leaves one, whose task ends
    /// with this error once `finish` is sent.
    fn connect_in_flight(
        sup: &mut Supervisor,
        error: BandError,
    ) -> (tokio::sync::oneshot::Sender<()>, mpsc::Sender<ConnState>) {
        let (finish, finished) = tokio::sync::oneshot::channel::<()>();
        let (progress_tx, progress) = mpsc::channel(4);
        sup.connecting = Some(Connecting {
            task: tokio::spawn(async move {
                let _ = finished.await;
                Err(error)
            }),
            progress,
            purpose: Purpose::Connect,
        });
        sup.state = ConnState::Scanning;
        (finish, progress_tx)
    }

    /// Cancel asks for a disconnect. The connect is not stopped part way,
    /// which would skip the cleanup that releases the band: it runs to its
    /// end, and the radio goes idle then, with no retry.
    #[tokio::test]
    async fn cancel_while_connecting_lets_the_connect_run_to_its_end() {
        let (mut sup, cmd_tx, _status_rx) = test_supervisor();
        sup.standing = standing(Phase::Connecting, Reason::You, ago(3));
        let (finish, _progress) = connect_in_flight(&mut sup, BandError::BandNotFound);

        cmd_tx.send(Command::Disconnect).await.unwrap();
        assert!(sup.wait_for_connect().await);
        assert!(sup.connecting.is_some(), "the connect was stopped part way");
        assert_eq!(sup.state, ConnState::Scanning, "the radio is the connect's until it lands");
        assert_eq!(sup.standing.state, Phase::Disconnected, "the window shows the cancel at once");
        assert_eq!(sup.standing.reason, Reason::You);
        let cancelled = sup.standing;

        finish.send(()).unwrap();
        assert!(sup.wait_for_connect().await);
        assert!(sup.connecting.is_none());
        assert_eq!(sup.state, ConnState::Idle, "a cancelled connect was retried");
        assert_eq!(sup.attempt, 0);
        assert_eq!(sup.standing, cancelled);
        assert_eq!(next_action(sup.desired(), &sup.state, Instant::now()), Action::Park);
    }

    /// Stop trying while a retry's attempt runs: the attempt runs to its end,
    /// and the band stays away, since the drop.
    #[tokio::test]
    async fn stop_trying_lets_a_retry_attempt_run_to_its_end() {
        let (mut sup, cmd_tx, _status_rx) = test_supervisor();
        let dropped = standing(Phase::Retrying, Reason::Dropped, ago(40));
        sup.standing = dropped;
        sup.attempt = 2;
        let (finish, _progress) = connect_in_flight(&mut sup, BandError::BandNotFound);

        cmd_tx.send(Command::StopTrying).await.unwrap();
        assert!(sup.wait_for_connect().await);
        assert!(sup.connecting.is_some(), "the attempt was stopped part way");
        let stopped = standing(Phase::Disconnected, Reason::Dropped, dropped.since);
        assert_eq!(sup.standing, stopped);

        finish.send(()).unwrap();
        assert!(sup.wait_for_connect().await);
        assert_eq!(sup.state, ConnState::Idle);
        assert_eq!(sup.attempt, 0);
        assert_eq!(sup.standing, stopped);
    }

    /// A first connect that fails is retried, since it failed, and the
    /// front ends hear the retry.
    #[tokio::test]
    async fn a_connect_that_fails_is_retried_since_it_failed() {
        let (mut sup, _cmd_tx, mut status_rx) = test_supervisor();
        let (finish, _progress) = connect_in_flight(&mut sup, BandError::BandNotFound);
        finish.send(()).unwrap();

        assert!(sup.wait_for_connect().await);

        assert_eq!(sup.standing.state, Phase::Retrying);
        assert_eq!(sup.standing.reason, Reason::Failed);
        assert!(matches!(sup.state, ConnState::Backoff { attempt: 1, .. }), "{:?}", sup.state);
        let mut told = None;
        while let Ok(status) = status_rx.try_recv() {
            if let Status::State(state, standing) = status {
                told = Some((state, standing));
            }
        }
        assert_eq!(told, Some((sup.state.clone(), sup.standing)));
    }

    // --- what the socket answers ---------------------------------------------

    /// `status` answers the radio's word for a person, then where the
    /// connection stands, as data.
    #[tokio::test]
    async fn the_status_request_answers_where_the_connection_stands() {
        let (mut sup, _cmd_tx, _status_rx) = test_supervisor();
        assert!(sup.apply(Some(Command::Disconnect)).await);

        let replies = ask(&mut sup, Request::Status).await;
        assert_eq!(replies.len(), 3, "{replies:?}");
        assert_eq!(
            replies[0],
            Reply::Line {
                text: "state disconnected".to_string()
            }
        );
        let Reply::Data { json } = &replies[1] else {
            panic!("expected the standing as data, got {:?}", replies[1]);
        };
        let told: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(told["state"], "disconnected");
        assert_eq!(told["reason"], "you");
        assert_eq!(told["since"], local_time(sup.standing.since));
        assert_eq!(replies[2], Reply::Done);
    }

    /// `device` gains the band's name and when the link came up, and keeps
    /// everything it answered before.
    #[tokio::test]
    async fn the_device_answer_names_the_band_and_when_it_connected() {
        let (mut sup, _cmd_tx, _status_rx) = test_supervisor();
        let up_at = ago(90);
        sup.standing = standing(Phase::Connected, Reason::You, up_at);

        let json = sup.device_json(Some("Xiaomi Smart Band 7".to_string()));
        assert_eq!(json["name"], "Xiaomi Smart Band 7");
        assert_eq!(json["connectedSince"], local_time(up_at));
        for field in ["mac", "serial", "hardware", "firmware", "battery"] {
            assert!(json.get(field).is_some(), "{field} is gone: {json}");
        }

        // Away, there is no time to give, and with no band paired, no name.
        sup.standing = sup
            .standing
            .after(Happened::LinkLost { retry: true }, SystemTime::now());
        let json = sup.device_json(None);
        assert!(json["connectedSince"].is_null(), "{json}");
        assert!(json["name"].is_null(), "{json}");
    }

    /// A change is told once, and told again only when asked for.
    #[tokio::test]
    async fn each_change_is_told_once() {
        let (mut sup, _cmd_tx, mut status_rx) = test_supervisor();
        let mut told = || {
            let mut states = 0;
            while let Ok(status) = status_rx.try_recv() {
                states += usize::from(matches!(status, Status::State(..)));
            }
            states
        };

        assert!(sup.apply(Some(Command::Disconnect)).await);
        assert_eq!(told(), 1);
        assert!(sup.apply(Some(Command::Disconnect)).await);
        assert_eq!(told(), 0, "a second disconnect changed nothing");
        assert!(sup.apply(Some(Command::ReportState)).await);
        assert_eq!(told(), 1, "asked for, it is told again");
    }

    // --- settings that apply at once ------------------------------------------

    /// A config with both settings on, and one action.
    const BOTH_ON: &str = "[settings]\nauto_reconnect = true\nnew_app_on_top = true\n\n\
                           [[actions]]\nname = \"Open Music\"\nsignal = \"bb03\"\nshell = \"open -a Music\"\n";

    /// A supervisor that follows a config file of its own, as the window's
    /// follows the live one. The file starts as `text`.
    fn following_a_file(
        text: &str,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        Supervisor,
        mpsc::Receiver<Status>,
    ) {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("config.toml");
        std::fs::write(&path, text).unwrap();
        let (mut sup, _cmd_tx, status_rx) = test_supervisor();
        sup.config_file = Some(path.clone());
        let sup = sup.following(&Config::load_from(&path).unwrap());
        (folder, path, sup, status_rx)
    }

    /// One setting changed in the file, by the write the window's
    /// `settings_set` makes.
    fn set_in_file(path: &std::path::Path, change: SettingsChange) {
        Config::change_settings_at(path, &change).unwrap();
    }

    fn auto_reconnect(on: bool) -> SettingsChange {
        SettingsChange {
            auto_reconnect: Some(on),
            ..SettingsChange::default()
        }
    }

    /// What the supervisor said since the last call: its notices alone.
    fn notices(status_rx: &mut mpsc::Receiver<Status>) -> Vec<String> {
        let mut said = Vec::new();
        while let Ok(status) = status_rx.try_recv() {
            if let Status::Notice(text) = status {
                said.push(text);
            }
        }
        said
    }

    /// A band that has been connected for a minute.
    fn connected(sup: &mut Supervisor) {
        sup.standing = standing(Phase::Connected, Reason::You, ago(60));
        sup.state = ConnState::Ready;
    }

    /// A pause before this retry.
    fn pausing(attempt: u32) -> ConnState {
        ConnState::Backoff {
            until: Instant::now() + Duration::from_secs(30),
            attempt,
        }
    }

    /// T-007's acceptance: turn off auto reconnect in the window, drop the
    /// link, and see Connection lost rather than Reconnecting, with no
    /// restart. The window writes the file, and the drop reads it.
    #[tokio::test]
    async fn a_change_to_the_file_reaches_the_supervisor() {
        let (_folder, path, mut sup, mut status_rx) = following_a_file(BOTH_ON);
        connected(&mut sup);

        set_in_file(&path, auto_reconnect(false));
        sup.link_lost(BandError::StreamClosed).await;

        assert_eq!(
            sup.standing.state,
            Phase::Disconnected,
            "Connection lost, not Reconnecting"
        );
        assert_eq!(sup.standing.reason, Reason::Dropped);
        assert_eq!(sup.state, ConnState::Idle);
        assert_eq!(
            next_action(sup.desired(), &sup.state, Instant::now()),
            Action::Park
        );
        assert_eq!(notices(&mut status_rx), [AUTO_RECONNECT_OFF]);

        // On again, and the next drop is tried again.
        set_in_file(&path, auto_reconnect(true));
        connected(&mut sup);
        sup.link_lost(BandError::StreamClosed).await;

        assert_eq!(sup.standing.state, Phase::Retrying, "Reconnecting");
        assert_eq!(sup.standing.reason, Reason::Dropped);
        assert!(
            matches!(sup.state, ConnState::Backoff { attempt: 1, .. }),
            "{:?}",
            sup.state
        );
        assert_eq!(notices(&mut status_rx), [AUTO_RECONNECT_ON]);
    }

    /// Auto reconnect off while BandBridge tries again after a drop: that
    /// retry is the reconnect the setting governs, so it ends, as Stop trying
    /// ends it, and the band stays away since the drop. A first connect that
    /// failed is not a drop, and goes on. Turning the setting on again
    /// revives nothing: only a person's ask for the link leaves disconnected.
    #[tokio::test]
    async fn auto_reconnect_off_ends_a_retry_after_a_drop_and_nothing_else() {
        let (_folder, path, mut sup, mut status_rx) = following_a_file(BOTH_ON);
        let dropped = standing(Phase::Retrying, Reason::Dropped, ago(40));
        sup.standing = dropped;
        sup.attempt = 2;
        sup.state = pausing(2);

        set_in_file(&path, auto_reconnect(false));
        // The window tells the band task, which looks at once.
        assert!(sup.apply(Some(Command::ConfigChanged)).await);

        let lost = standing(Phase::Disconnected, Reason::Dropped, dropped.since);
        assert_eq!(sup.standing, lost);
        assert_eq!(
            sup.state,
            ConnState::Idle,
            "a pause for a retry that is not coming"
        );
        assert_eq!(sup.attempt, 0);
        assert_eq!(
            next_action(sup.desired(), &sup.state, Instant::now()),
            Action::Park
        );
        assert_eq!(
            notices(&mut status_rx),
            [AUTO_RECONNECT_OFF, STOPPED_FOR_AUTO_RECONNECT]
        );

        set_in_file(&path, auto_reconnect(true));
        assert!(sup.apply(Some(Command::ConfigChanged)).await);
        assert_eq!(sup.standing, lost, "a setting revived a disconnect");
        assert_eq!(
            next_action(sup.desired(), &sup.state, Instant::now()),
            Action::Park
        );
        assert_eq!(notices(&mut status_rx), [AUTO_RECONNECT_ON]);

        let failing = standing(Phase::Retrying, Reason::Failed, ago(10));
        sup.standing = failing;
        sup.state = pausing(3);
        set_in_file(&path, auto_reconnect(false));
        assert!(sup.apply(Some(Command::ConfigChanged)).await);
        assert_eq!(
            sup.standing, failing,
            "a first connect that failed stopped trying"
        );
        assert!(
            matches!(sup.state, ConnState::Backoff { attempt: 3, .. }),
            "{:?}",
            sup.state
        );
        assert_eq!(notices(&mut status_rx), [AUTO_RECONNECT_OFF]);
    }

    /// New app on top and the actions are taken up at the next look, and
    /// said once. Appearance is the window's alone, and the band task says
    /// nothing of it.
    #[tokio::test]
    async fn new_app_on_top_and_the_actions_are_taken_up_at_the_next_look() {
        let (_folder, path, mut sup, mut status_rx) = following_a_file(BOTH_ON);
        assert!(sup.settings.new_app_on_top);
        assert_eq!(sup.actions.len(), 1);

        set_in_file(
            &path,
            SettingsChange {
                new_app_on_top: Some(false),
                appearance: Some(Appearance::Dark),
                ..SettingsChange::default()
            },
        );
        sup.follow_changes().await;
        assert!(!sup.settings.new_app_on_top);
        assert_eq!(notices(&mut status_rx), [NEW_APP_ON_TOP_OFF]);

        // An action added to the file fires from the next look, as before.
        let mut config = Config::load_from(&path).unwrap();
        config.actions.push(crate::config::Action {
            name: "Say hi".to_string(),
            signal: "bb04".to_string(),
            does: crate::config::Does::Shell("say hi".to_string()),
        });
        config.save_to(&path).unwrap();
        sup.follow_changes().await;
        assert_eq!(sup.actions, config.actions);
        assert_eq!(notices(&mut status_rx), ["2 action(s) loaded"]);

        // A look at a file that has not changed since says nothing.
        sup.follow_changes().await;
        assert_eq!(notices(&mut status_rx), Vec::<String>::new());
    }

    /// A file that does not load changes nothing, and says why once.
    #[tokio::test]
    async fn a_file_that_does_not_load_changes_nothing() {
        let (_folder, path, mut sup, mut status_rx) = following_a_file(BOTH_ON);
        let before = (sup.settings, sup.actions.clone());

        std::fs::write(
            &path,
            BOTH_ON.replace("auto_reconnect = true", "auto_reconnect = \"no\""),
        )
        .unwrap();
        sup.follow_changes().await;

        assert_eq!((sup.settings, sup.actions.clone()), before);
        let said = notices(&mut status_rx);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].starts_with(
                "the config file was not loaded, so the actions and settings stay as they were"
            ),
            "{said:?}"
        );
        assert!(said[0].contains("line 2"), "{said:?}");
        sup.follow_changes().await;
        assert_eq!(notices(&mut status_rx), Vec::<String>::new(), "said again");
    }

    /// `listen --no-auto-reconnect` holds for the whole run. The file says
    /// reconnect, and is read again for another change, and still a drop
    /// stays down.
    #[tokio::test]
    async fn no_auto_reconnect_on_the_command_line_wins_over_the_file() {
        let (_folder, path, sup, mut status_rx) = following_a_file(BOTH_ON);
        let mut sup = sup.never_reconnecting(true);
        connected(&mut sup);

        set_in_file(
            &path,
            SettingsChange {
                new_app_on_top: Some(false),
                ..SettingsChange::default()
            },
        );
        sup.link_lost(BandError::StreamClosed).await;

        assert_eq!(sup.standing.state, Phase::Disconnected);
        assert_eq!(sup.standing.reason, Reason::Dropped);
        assert_eq!(
            notices(&mut status_rx),
            [NEW_APP_ON_TOP_OFF],
            "auto reconnect was never on"
        );
    }

    /// A change made in the file by hand counts from the next thing that
    /// happens, whatever it is: here, a status request on the socket, on a
    /// supervisor that waits with no band paired.
    #[tokio::test]
    async fn a_change_by_hand_counts_from_the_next_thing_that_happens() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("config.toml");
        std::fs::write(&path, BOTH_ON).unwrap();
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(8);
        let (status_tx, mut status_rx) = mpsc::channel::<Status>(64);
        let (req_tx, req_rx) = mpsc::channel::<Incoming>(8);
        let mut sup = Supervisor::new(None, test_options(), cmd_rx, status_tx);
        sup.config_file = Some(path.clone());
        sup.load_key = no_keychain;
        let sup = sup
            .serving(req_rx)
            .following(&Config::load_from(&path).unwrap());

        let drive = async {
            set_in_file(&path, auto_reconnect(false));
            let (reply, mut replies) = mpsc::channel(8);
            req_tx
                .send(Incoming {
                    request: Request::Status,
                    reply,
                })
                .await
                .unwrap();
            while let Some(reply) = replies.recv().await {
                if reply == Reply::Done {
                    break;
                }
            }
            cmd_tx.send(Command::Shutdown).await.unwrap();
        };
        let (ran, ()) = tokio::join!(sup.run(), drive);
        ran.unwrap();

        assert_eq!(notices(&mut status_rx), [AUTO_RECONNECT_OFF]);
    }

    // --- the event log ----------------------------------------------------------

    use crate::event::{ENDPOINT_APP, ENDPOINT_HTTP};
    use crate::protocol::chunk::ZeppMessage;

    /// A message on this endpoint with these bytes, as the band sends it.
    fn message(endpoint: u16, payload: &[u8]) -> ZeppMessage {
        ZeppMessage {
            endpoint,
            payload: payload.to_vec(),
            encrypted: false,
        }
    }

    /// An action that runs a shell command. None of these tests uses a
    /// keystroke, which would type into whatever window is in front.
    fn shell_action(name: &str, signal: &str, line: &str) -> crate::config::Action {
        crate::config::Action {
            name: name.to_string(),
            signal: signal.to_string(),
            does: crate::config::Does::Shell(line.to_string()),
        }
    }

    /// Everything told to the front ends and not yet read.
    fn told(status_rx: &mut mpsc::Receiver<Status>) -> Vec<Status> {
        let mut all = Vec::new();
        while let Ok(status) = status_rx.try_recv() {
            all.push(status);
        }
        all
    }

    /// The next thing told, waited for: an action's result comes from a task
    /// of its own.
    async fn next_told(status_rx: &mut mpsc::Receiver<Status>) -> Status {
        tokio::time::timeout(Duration::from_secs(5), status_rx.recv())
            .await
            .expect("told in time")
            .expect("the channel is open")
    }

    fn pressed(status: &Status) -> &Press {
        match status {
            Status::Pressed(press) => press,
            other => panic!("expected a press, got {other:?}"),
        }
    }

    /// A press whose signal no action answers is still a press: no action,
    /// and no result to wait for. A terminal prints its bytes, as before.
    #[tokio::test]
    async fn a_press_that_fires_nothing_is_still_a_press() {
        let (mut sup, _cmd_tx, mut status_rx) = test_supervisor();
        let before = SystemTime::now();
        sup.heard(message(ENDPOINT_APP, &[0xbb, 0x09])).await;

        let told = told(&mut status_rx);
        assert_eq!(told.len(), 2, "{told:?}");
        assert!(matches!(told[0], Status::Event(BandEvent::Unknown { .. })));
        let press = pressed(&told[1]);
        assert_eq!(press.id, 1);
        assert_eq!(press.signal, "bb09");
        assert!(press.at >= before, "the time the press came");
        assert_eq!((&press.action, &press.result), (&None, &None));

        assert_eq!(told[0].terminal_line().as_deref(), Some("[app] bb09"));
        assert_eq!(told[1].terminal_line(), None);
    }

    /// Several actions may answer one signal (D6). A press runs them one by
    /// one, in list order: each starts only when the one before has ended.
    /// Each is told at the press, in that order, under a number of its own,
    /// and again with its result.
    #[tokio::test]
    async fn a_press_runs_every_action_on_its_signal_one_by_one_in_list_order() {
        use crate::platform::actions::lines;
        let folder = tempfile::tempdir().unwrap();
        let trace = folder.path().join("trace");
        let (mut sup, _cmd_tx, mut status_rx) = test_supervisor();
        let first = format!("{}; {}", lines::pause(300), lines::append("first", &trace));
        sup.actions = vec![
            shell_action("First", "bb03", &first),
            shell_action("Elsewhere", "bb04", &lines::append("elsewhere", &trace)),
            shell_action("Second", "BB03", &lines::append("second", &trace)),
        ];
        sup.heard(message(ENDPOINT_APP, &[0xbb, 0x03])).await;

        let told_now = told(&mut status_rx);
        assert_eq!(
            told_now.len(),
            3,
            "the band's event, then one press per action"
        );
        let first = pressed(&told_now[1]).clone();
        let second = pressed(&told_now[2]).clone();
        assert_eq!(first.action.as_deref(), Some("First"));
        assert_eq!(second.action.as_deref(), Some("Second"));
        assert_eq!(second.id, first.id + 1);
        assert_eq!((second.at, second.signal.as_str()), (first.at, "bb03"));
        assert_eq!((&first.result, &second.result), (&None, &None));

        for (want, id) in [("First", first.id), ("Second", second.id)] {
            let later = next_told(&mut status_rx).await;
            let Status::Finished(finished) = &later else {
                panic!("expected a result, got {later:?}");
            };
            assert_eq!((finished.action.as_deref(), finished.id), (Some(want), id));
            assert!(matches!(
                finished.result,
                Some(Outcome::Exit { code: 0, .. })
            ));
        }
        let ran = std::fs::read_to_string(&trace).unwrap();
        assert_eq!(
            ran, "first\nsecond\n",
            "the second started before the first ended"
        );
    }

    /// An action that takes time is told at the press, with no result, and
    /// again when it ends, under the same number and time, with its result:
    /// here a shell command's exit code. A terminal reads as it always has.
    #[tokio::test]
    async fn a_command_is_told_at_the_press_and_again_with_its_exit_code() {
        let (mut sup, _cmd_tx, mut status_rx) = test_supervisor();
        sup.actions = vec![shell_action("Say hi", "bb03", "exit 0")];
        sup.heard(message(ENDPOINT_APP, &[0xbb, 0x03])).await;

        let told_now = told(&mut status_rx);
        assert_eq!(told_now.len(), 2, "{told_now:?}");
        assert_eq!(told_now[0].terminal_line().as_deref(), Some("[app] bb03"));
        let press = pressed(&told_now[1]).clone();
        assert_eq!(press.action.as_deref(), Some("Say hi"));
        assert_eq!(press.result, None, "the command still runs");
        assert_eq!(
            told_now[1].terminal_line().as_deref(),
            Some("[info]   fired Say hi (shell: exit 0)")
        );

        let later = next_told(&mut status_rx).await;
        let Status::Finished(finished) = &later else {
            panic!("expected the result, got {later:?}");
        };
        assert_eq!((finished.id, finished.at), (press.id, press.at));
        assert_eq!(finished.signal, "bb03");
        assert_eq!(
            finished.result,
            Some(Outcome::Exit {
                code: 0,
                error: None
            })
        );
        assert_eq!(
            later.terminal_line().as_deref(),
            Some("[info]   Say hi: done")
        );
        assert!(told(&mut status_rx).is_empty(), "told more than twice");
    }

    /// A keystroke is over when the press is told, so it is told once, with
    /// its result. Nothing follows it.
    #[tokio::test]
    async fn a_keystroke_is_told_once_with_its_result() {
        let (mut sup, _cmd_tx, mut status_rx) = test_supervisor();
        let fired = Fired {
            started: Started::Finished(Outcome::Done),
            name: "Page up".to_string(),
            does: "page_up".to_string(),
        };
        let bytes = BandEvent::from_message(message(ENDPOINT_APP, &[0xbb, 0x02]));
        sup.tell_press(
            bytes,
            "bb02".to_string(),
            SystemTime::now(),
            Some(fired),
            Vec::new(),
        )
        .await;

        let told_now = told(&mut status_rx);
        assert_eq!(told_now.len(), 2, "{told_now:?}");
        let press = pressed(&told_now[1]);
        assert_eq!(press.action.as_deref(), Some("Page up"));
        assert_eq!(press.result, Some(Outcome::Done));
        assert_eq!(
            told_now[1].terminal_line().as_deref(),
            Some("[info]   fired Page up (page_up)")
        );

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            told(&mut status_rx).is_empty(),
            "a keystroke was told twice"
        );
    }

    /// A result is never told before its press, even when the action is over
    /// before the press is told: here a follower that reads slowly holds the
    /// press up, and the action's task could run on another thread meanwhile.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_result_is_never_told_before_its_press() {
        let (mut sup, _cmd_tx, mut status_rx) = test_supervisor();
        let (follower, mut follower_rx) = mpsc::channel(1);
        follower
            .send(Reply::Line {
                text: "unread".to_string(),
            })
            .await
            .unwrap();
        sup.followers.push(Answer(follower));

        let over = tokio::spawn(async {
            Outcome::Exit {
                code: 0,
                error: None,
            }
        });
        while !over.is_finished() {
            tokio::task::yield_now().await;
        }
        let fired = Fired {
            started: Started::Running(over),
            name: "Say hi".to_string(),
            does: "shell: true".to_string(),
        };
        let bytes = BandEvent::from_message(message(ENDPOINT_APP, &[0xbb, 0x03]));
        let reader = async {
            // Long enough for a result told early to reach the channel.
            tokio::time::sleep(Duration::from_millis(150)).await;
            for _ in 0..3 {
                follower_rx.recv().await.expect("a line for the follower");
            }
        };
        tokio::join!(
            sup.tell_press(
                bytes,
                "bb03".to_string(),
                SystemTime::now(),
                Some(fired),
                Vec::new()
            ),
            reader
        );

        let mut order = Vec::new();
        for _ in 0..3 {
            order.push(match next_told(&mut status_rx).await {
                Status::Event(_) => "bytes",
                Status::Pressed(_) => "press",
                Status::Finished(_) => "result",
                other => panic!("told {other:?}"),
            });
        }
        assert_eq!(order, ["bytes", "press", "result"]);
    }

    /// `follow` hears a press as it always has: its bytes and what it fired.
    /// It never heard an action's result, and it still does not.
    #[tokio::test]
    async fn a_follower_hears_a_press_as_before() {
        let (mut sup, _cmd_tx, mut status_rx) = test_supervisor();
        let (follower, mut follower_rx) = mpsc::channel(32);
        sup.followers.push(Answer(follower));
        sup.actions = vec![shell_action("Say hi", "bb03", "exit 0")];

        sup.heard(message(ENDPOINT_APP, &[0xbb, 0x03])).await;
        sup.heard(message(ENDPOINT_APP, &[0xbb, 0x05])).await;
        loop {
            if matches!(next_told(&mut status_rx).await, Status::Finished(_)) {
                break;
            }
        }

        let mut lines = Vec::new();
        while let Ok(Reply::Line { text }) = follower_rx.try_recv() {
            lines.push(text);
        }
        assert_eq!(
            lines,
            [
                "[app] bb03",
                "[info]   fired Say hi (shell: exit 0)",
                "[app] bb05"
            ]
        );
    }

    /// Nothing but a press becomes a record of what the band sends: not its
    /// own traffic, not an answer to a list request, not a battery report,
    /// not the stock music screen, and not the band's platform messages on
    /// the app endpoint. What a terminal showed before, it still shows.
    #[tokio::test]
    async fn nothing_the_band_sends_but_a_press_becomes_a_record() {
        let mut list_reply = vec![0x02, 0x00, 0x01];
        list_reply.resize(24, 0);
        let housekeeping = || {
            [
                message(crate::protocol::battery::ENDPOINT, &[0x04, 0x0f, 74, 0]),
                message(ENDPOINT_APP, &list_reply),
                message(ENDPOINT_HTTP, &[0x01, 0x02, 0x03]),
                message(ENDPOINT_HTTP, &[0xbb, 0x01]),
            ]
        };
        let shown = || {
            [
                message(crate::protocol::music::ENDPOINT_MUSIC, &[0x05, 0x05]),
                message(0x0010, &[0xbb, 0x01]),
                message(ENDPOINT_APP, &[0x01, 0x02]),
            ]
        };

        for raw in [false, true] {
            let (mut sup, _cmd_tx, mut status_rx) = test_supervisor();
            sup.opts.raw = raw;
            sup.actions = vec![shell_action("Say hi", "bb01", "exit 0")];
            for message in housekeeping().into_iter().chain(shown()) {
                sup.heard(message).await;
            }

            let told = told(&mut status_rx);
            assert!(
                told.iter().all(|status| matches!(status, Status::Event(_))),
                "raw {raw}: {told:?}"
            );
            let lines: Vec<String> = told.iter().filter_map(Status::terminal_line).collect();
            let expected = if raw { 7 } else { 3 };
            assert_eq!(lines.len(), expected, "raw {raw}: {lines:?}");
            assert!(
                lines.contains(&"[music] volume_up".to_string()),
                "{lines:?}"
            );
            assert_eq!(sup.records, 0, "raw {raw}: a record was numbered");
            assert_eq!(
                sup.battery.map(|(battery, _)| battery.percent),
                Some(74),
                "the battery is remembered, and not told"
            );
        }
    }

    /// An install that worked is logged by the app's name, once, and the
    /// person who asked hears it went on.
    #[tokio::test]
    async fn an_app_that_went_on_is_logged_by_its_name() {
        let (mut sup, _cmd_tx, mut status_rx) = test_supervisor();
        let (reply, mut replies) = mpsc::channel(8);
        let before = SystemTime::now();

        sup.installed(Ok("testbandapp".to_string()), false, &Answer(reply))
            .await;

        let told = told(&mut status_rx);
        assert_eq!(told.len(), 1, "{told:?}");
        let Status::AppChanged(record) = &told[0] else {
            panic!("expected the app, got {:?}", told[0]);
        };
        assert_eq!(
            (record.id, record.change, record.app.as_str()),
            (1, Change::Installed, "testbandapp")
        );
        assert!(record.at >= before);
        assert_eq!(
            told[0].terminal_line(),
            None,
            "a terminal prints nothing new"
        );

        assert_eq!(
            replies.try_recv().unwrap(),
            Reply::Line {
                text: "Installed.".to_string()
            }
        );
        assert_eq!(replies.try_recv().unwrap(), Reply::Done);
    }

    /// An install that failed installed nothing, so nothing is logged. Here
    /// it fails for real, through the request: the band is not connected.
    #[tokio::test]
    async fn an_install_that_failed_logs_nothing() {
        let (mut sup, _cmd_tx, mut status_rx) = test_supervisor();
        // A package with no manifest near it, so nothing learns its name.
        let folder = tempfile::tempdir().unwrap();
        let dist = folder.path().join("app/dist");
        std::fs::create_dir_all(&dist).unwrap();
        let package = dist.join("slides.bin");
        std::fs::write(&package, b"not a real package").unwrap();

        let replies = ask(
            &mut sup,
            Request::Install {
                file: package.display().to_string(),
            },
        )
        .await;
        assert_eq!(
            failed(&replies),
            "install failed: the band is not connected. Connect first."
        );
        assert!(told(&mut status_rx).is_empty());

        let (reply, _replies) = mpsc::channel(8);
        let refused = BandError::Install("the band refused it".to_string());
        sup.installed(Err(refused), false, &Answer(reply)).await;
        assert!(told(&mut status_rx).is_empty());
        assert_eq!(sup.records, 0);
    }

    /// Whoever asked for an install hears the upload go: a line each time
    /// its percentage moves, as the command line prints it. The band app
    /// editor turns these into the percentage on its Install button.
    #[tokio::test]
    async fn an_install_tells_its_caller_each_percent_it_sends() {
        let (reply, mut replies) = mpsc::channel(32);
        let mut tell = tell_progress(&Answer(reply));
        for sent in [244, 300, 30_100, 48_213] {
            tell(sent, 48_213);
        }
        let mut heard = Vec::new();
        while let Ok(reply) = replies.try_recv() {
            heard.push(reply);
        }
        let line = |text: &str| Reply::Line {
            text: text.to_string(),
        };
        assert_eq!(heard, [line("0%"), line("62%"), line("100%")]);
    }

    // --- new app on top (D10) ----------------------------------------------------

    /// The band's menu after an install: it dropped testbandapp third, after
    /// a screen of its own, and keeps BandArrows out of view.
    fn a_menu_the_band_left() -> Vec<crate::shortcuts::DisplayItem> {
        use crate::shortcuts::{DisplayItem, Section};
        let item = |id: &str, section, position| DisplayItem {
            id: id.to_string(),
            section,
            position,
            flags: 0,
            extra: vec![],
        };
        vec![
            item("58E537B5", Section::Main, 0),
            item("0000000B", Section::Main, 1),
            item("10BD37D4", Section::Main, 2),
            item("00000013", Section::Main, 3),
            item("58E538D5", Section::Disabled, 0),
        ]
    }

    /// An install over the link that went on, as `install_here` leaves it:
    /// the id from the app's manifest waits to be lifted.
    async fn an_install_went_on(sup: &mut Supervisor) {
        sup.newest_app = Some("10BD37D4".to_string());
        let (reply, _replies) = mpsc::channel(8);
        sup.installed(Ok("testbandapp".to_string()), true, &Answer(reply))
            .await;
    }

    fn new_app_on_top(on: bool) -> SettingsChange {
        SettingsChange {
            new_app_on_top: Some(on),
            ..SettingsChange::default()
        }
    }

    /// D10, the switch on: an install from here puts the new app first in
    /// the menu, once, as the link comes back. Every other screen stays where
    /// it was: the lift moves the new app and nothing else.
    #[tokio::test]
    async fn new_app_on_top_on_lifts_an_app_installed_here_once() {
        let (_folder, _path, mut sup, _status_rx) = following_a_file(BOTH_ON);
        an_install_went_on(&mut sup).await;

        let newest = sup
            .lift_owed()
            .expect("the link after the install lifts the app");
        assert_eq!(newest, "10BD37D4");
        assert_eq!(
            crate::screens::lift_order(&a_menu_the_band_left(), &newest),
            Some(vec![
                "10BD37D4".to_string(),
                "58E537B5".to_string(),
                "0000000B".to_string(),
                "00000013".to_string(),
            ])
        );
        assert_eq!(sup.lift_owed(), None, "the next connect lifts nothing");
    }

    /// A connect alone never touches the menu, whatever the switch says, so
    /// an order the person arranged stays.
    #[tokio::test]
    async fn a_connect_leaves_the_menu_as_the_person_arranged_it() {
        let (_folder, _path, mut sup, _status_rx) = following_a_file(BOTH_ON);
        assert!(sup.settings.new_app_on_top);
        assert_eq!(sup.lift_owed(), None);
    }

    /// D10, the switch off: an install leaves the menu as the band arranges
    /// it. The install reads the switch from the file as it ends, and a switch
    /// turned on later lifts no app it did not see installed.
    #[tokio::test]
    async fn new_app_on_top_off_leaves_the_menu_as_the_band_arranges_it() {
        let (_folder, path, mut sup, _status_rx) = following_a_file(BOTH_ON);
        set_in_file(&path, new_app_on_top(false));
        an_install_went_on(&mut sup).await;

        assert!(!sup.settings.new_app_on_top, "the install read the file");
        assert_eq!(sup.lift_owed(), None, "the menu stays as the band left it");

        set_in_file(&path, new_app_on_top(true));
        sup.follow_changes().await;
        assert_eq!(
            sup.lift_owed(),
            None,
            "turning the switch on is not an install"
        );
    }

    /// A switch turned off before the link comes back cancels the lift.
    #[tokio::test]
    async fn a_switch_turned_off_before_the_link_is_back_lifts_nothing() {
        let (_folder, path, mut sup, _status_rx) = following_a_file(BOTH_ON);
        an_install_went_on(&mut sup).await;
        set_in_file(&path, new_app_on_top(false));
        sup.follow_changes().await;
        assert_eq!(sup.lift_owed(), None);
    }

    /// An install that failed lifts nothing, with the switch on too.
    #[tokio::test]
    async fn an_install_that_failed_lifts_nothing() {
        let (_folder, _path, mut sup, _status_rx) = following_a_file(BOTH_ON);
        sup.newest_app = Some("10BD37D4".to_string());
        let (reply, _replies) = mpsc::channel(8);
        let refused = BandError::Install("the band refused it".to_string());
        sup.installed(Err(refused), true, &Answer(reply)).await;
        assert_eq!(sup.lift_owed(), None);
    }

    /// An app that went off the band is logged by its name, or by its id
    /// when it has none, and whoever asked hears the line they always heard.
    #[tokio::test]
    async fn an_app_that_went_off_is_logged_by_its_name_or_its_id() {
        let (mut sup, _cmd_tx, mut status_rx) = test_supervisor();
        let app = |name: Option<&str>, id: &str, version: &str| crate::apps::Entry {
            id: id.to_string(),
            number: u32::from_str_radix(id, 16).ok(),
            version: version.to_string(),
            name: name.map(str::to_string),
            is_watchface: false,
            in_use: false,
        };

        let lines = sup
            .removed(&app(Some("BandArrows"), "58E538D5", "1.0.4"))
            .await;
        assert_eq!(lines, ["removed BandArrows (58E538D5) version 1.0.4"]);
        let lines = sup.removed(&app(None, "2D38", "1.0")).await;
        assert_eq!(lines, ["removed 2D38 version 1.0"]);

        let logged: Vec<(Change, String)> = told(&mut status_rx)
            .into_iter()
            .map(|status| match status {
                Status::AppChanged(record) => (record.change, record.app),
                other => panic!("expected an app, got {other:?}"),
            })
            .collect();
        assert_eq!(
            logged,
            [
                (Change::Removed, "BandArrows".to_string()),
                (Change::Removed, "2D38".to_string())
            ]
        );
    }

    /// A removal that could not happen removed nothing, so nothing is logged.
    #[tokio::test]
    async fn a_removal_that_failed_logs_nothing() {
        let (mut sup, _cmd_tx, mut status_rx) = test_supervisor();
        let replies = ask(
            &mut sup,
            Request::Apps {
                remove: Some("58E538D5".to_string()),
                all: false,
            },
        )
        .await;
        assert_eq!(
            failed(&replies),
            "apps: the band is not connected. Connect first."
        );
        assert!(told(&mut status_rx).is_empty());
    }

    /// An app installed from a package with no manifest near it goes by the
    /// package's file name: honest about what went on, and never a name
    /// made up.
    #[test]
    fn an_installed_app_goes_by_its_name_or_its_file() {
        let package = Path::new("/Users/ada/apps/slides/dist/slides.bin");
        assert_eq!(shown_app_name(package, Some("Slides")), "Slides");
        assert_eq!(shown_app_name(package, None), "slides.bin");
        assert_eq!(shown_app_name(Path::new("/"), None), "/");
    }

    /// Presses and apps share one count, so each record has a number no other
    /// record in the run has.
    #[tokio::test]
    async fn every_record_takes_the_next_number() {
        let (mut sup, _cmd_tx, mut status_rx) = test_supervisor();
        sup.heard(message(ENDPOINT_APP, &[0xbb, 0x01])).await;
        sup.log_app(Change::Removed, "BandArrows".to_string()).await;
        sup.heard(message(ENDPOINT_APP, &[0xbb, 0x02])).await;

        let numbers: Vec<u64> = told(&mut status_rx)
            .iter()
            .filter_map(|status| match status {
                Status::Pressed(press) => Some(press.id),
                Status::AppChanged(app) => Some(app.id),
                _ => None,
            })
            .collect();
        assert_eq!(numbers, [1, 2, 3]);
    }

    /// The other lines a terminal prints are as they were.
    #[test]
    fn a_terminal_prints_the_rest_as_before() {
        let lines: Vec<Option<String>> = [
            Status::Failed("link lost: the band stopped answering".to_string()),
            Status::Notice("found Xiaomi Smart Band 7".to_string()),
            Status::State(
                ConnState::Ready,
                standing(Phase::Connected, Reason::You, ago(5)),
            ),
        ]
        .iter()
        .map(Status::terminal_line)
        .collect();
        assert_eq!(
            lines[0].as_deref(),
            Some("[error] link lost: the band stopped answering")
        );
        assert_eq!(
            lines[1].as_deref(),
            Some("[info] found Xiaomi Smart Band 7")
        );
        assert!(
            lines[2]
                .as_deref()
                .is_some_and(|line| line.starts_with("[state] ready · connected (you) since ")),
            "{lines:?}"
        );
    }

    // --- learn from the device ---------------------------------------------------

    /// A command that leaves a mark in `file` each time it runs, so a test
    /// can tell whether an action fired.
    fn marking(file: &std::path::Path) -> String {
        crate::platform::actions::lines::append("x", file)
    }

    /// The press the window's learn is waiting for, sent through the band
    /// task's own command channel, as the window sends it.
    async fn start_learning(sup: &mut Supervisor) -> oneshot::Receiver<Learned> {
        let (learner, learned) = oneshot::channel();
        assert!(sup.apply(Some(Command::Learn(learner))).await);
        learned
    }

    /// How a learn ended. A learn nobody answers fails the test, rather than
    /// hang it.
    async fn answered(learned: oneshot::Receiver<Learned>) -> Learned {
        tokio::time::timeout(Duration::from_secs(5), learned)
            .await
            .expect("the learn was answered in time")
            .expect("the band task answered the learn")
    }

    /// What the window's side of a learn heard back, with the same limit.
    async fn heard_back(
        waiting: tokio::task::JoinHandle<Result<Option<String>, String>>,
    ) -> Result<Option<String>, String> {
        tokio::time::timeout(Duration::from_secs(5), waiting)
            .await
            .expect("the window heard back in time")
            .expect("the window's side of the learn ran to its end")
    }

    /// Wait for the result of the press told last, so no action is left
    /// running when a test looks for its mark.
    async fn until_finished(status_rx: &mut mpsc::Receiver<Status>) -> Press {
        loop {
            if let Status::Finished(press) = next_told(status_rx).await {
                return press;
            }
        }
    }

    /// Learn from the device answers the next press with its signal, and
    /// fires nothing: not the action bound to the signal, and not a line of
    /// the event log. A terminal sees the band's bytes and what became of
    /// them. The learn takes one press, and the next fires as usual.
    #[tokio::test]
    async fn a_learn_takes_the_next_press_and_fires_nothing() {
        let (mut sup, _cmd_tx, mut status_rx) = test_supervisor();
        connected(&mut sup);
        let folder = tempfile::tempdir().unwrap();
        let mark = folder.path().join("fired");
        sup.actions = vec![shell_action("Mark", "bb01", &marking(&mark))];

        let learned = start_learning(&mut sup).await;
        sup.heard(message(ENDPOINT_APP, &[0xbb, 0x01])).await;
        assert_eq!(answered(learned).await, Learned::Signal("bb01".to_string()));

        let said = told(&mut status_rx);
        assert!(
            said.iter()
                .all(|status| !matches!(status, Status::Pressed(_) | Status::Finished(_))),
            "a learned press became a record: {said:?}"
        );
        let lines: Vec<String> = said.iter().filter_map(Status::terminal_line).collect();
        assert_eq!(
            lines,
            [
                "[app] bb01",
                "[info] the window learned bb01, so nothing fired"
            ]
        );
        // Long enough for a command that fired to have left its mark.
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!mark.exists(), "the action bound to bb01 fired");
        assert!(
            told(&mut status_rx).is_empty(),
            "{:?}",
            told(&mut status_rx)
        );

        sup.heard(message(ENDPOINT_APP, &[0xbb, 0x01])).await;
        let said = told(&mut status_rx);
        let press = pressed(&said[1]);
        assert_eq!(press.id, 1, "the learned press took a number");
        assert_eq!(press.action.as_deref(), Some("Mark"));
        until_finished(&mut status_rx).await;
        assert_eq!(
            std::fs::read_to_string(&mark).unwrap(),
            "x\n",
            "the next press fired once"
        );
    }

    /// Only a connected band sends a press, so a learn with the band away
    /// hears so at once, rather than wait a minute for nothing.
    #[tokio::test]
    async fn a_learn_with_the_band_away_hears_so_at_once() {
        let (mut sup, _cmd_tx, _status_rx) = test_supervisor();
        assert_ne!(sup.standing.state, Phase::Connected);
        let learned = start_learning(&mut sup).await;
        assert_eq!(
            answered(learned).await,
            Learned::Failed(NOT_CONNECTED_TO_LEARN.to_string())
        );
        assert!(sup.learner.is_none());
    }

    /// One learn at a time: a second takes the place of the first, which
    /// hears it was stopped, and the press goes to the second.
    #[tokio::test]
    async fn one_learn_at_a_time() {
        let (mut sup, _cmd_tx, _status_rx) = test_supervisor();
        connected(&mut sup);
        let first = start_learning(&mut sup).await;
        let second = start_learning(&mut sup).await;
        assert_eq!(answered(first).await, Learned::Stopped);
        sup.heard(message(ENDPOINT_APP, &[0xbb, 0x02])).await;
        assert_eq!(answered(second).await, Learned::Signal("bb02".to_string()));
    }

    /// A learn that was stopped, or that nobody waits for any more, takes no
    /// press: the next one fires as usual. A person who closed the editor
    /// never loses a press to it.
    #[tokio::test]
    async fn a_learn_nobody_waits_for_takes_no_press() {
        for how in ["stopped", "given up on"] {
            let (mut sup, _cmd_tx, mut status_rx) = test_supervisor();
            connected(&mut sup);
            sup.actions = vec![shell_action("Say hi", "bb01", "exit 0")];
            let mut learned = start_learning(&mut sup).await;
            if how == "stopped" {
                assert!(sup.apply(Some(Command::StopLearning)).await);
                assert_eq!(answered(learned).await, Learned::Stopped);
            } else {
                learned.close();
            }
            sup.heard(message(ENDPOINT_APP, &[0xbb, 0x01])).await;
            let said = told(&mut status_rx);
            assert_eq!(
                pressed(&said[1]).action.as_deref(),
                Some("Say hi"),
                "a learn {how} took the press: {said:?}"
            );
            until_finished(&mut status_rx).await;
        }
    }

    /// A band that goes away ends the learn: it drops, or a person lets it
    /// go. No press can come after that.
    #[tokio::test]
    async fn a_learn_ends_when_the_band_goes() {
        let (mut sup, _cmd_tx, _status_rx) = test_supervisor();
        connected(&mut sup);
        let learned = start_learning(&mut sup).await;
        sup.link_lost(BandError::StreamClosed).await;
        assert_eq!(
            answered(learned).await,
            Learned::Failed(WENT_BEFORE_A_PRESS.to_string())
        );

        connected(&mut sup);
        let learned = start_learning(&mut sup).await;
        assert!(sup.apply(Some(Command::Disconnect)).await);
        assert_eq!(
            answered(learned).await,
            Learned::Failed(WENT_BEFORE_A_PRESS.to_string())
        );
    }

    /// The window's side of a learn, with the band task: the signal, nothing
    /// when it was stopped, and the reason when no press can come.
    #[tokio::test]
    async fn the_window_hears_the_signal_nothing_or_why() {
        let (mut sup, cmd_tx, _status_rx) = test_supervisor();
        connected(&mut sup);
        let asking = cmd_tx.clone();
        let waiting = tokio::spawn(async move { learn(&asking, LEARN_LIMIT).await });
        let command = sup.commands.recv().await;
        assert!(sup.apply(command).await);
        sup.heard(message(ENDPOINT_APP, &[0xbb, 0x04])).await;
        assert_eq!(heard_back(waiting).await, Ok(Some("bb04".to_string())));

        let asking = cmd_tx.clone();
        let waiting = tokio::spawn(async move { learn(&asking, LEARN_LIMIT).await });
        let command = sup.commands.recv().await;
        assert!(sup.apply(command).await);
        assert!(sup.apply(Some(Command::StopLearning)).await);
        assert_eq!(
            heard_back(waiting).await,
            Ok(None),
            "a stopped learn shows nothing"
        );

        sup.standing = standing(Phase::Disconnected, Reason::You, ago(5));
        let asking = cmd_tx.clone();
        let waiting = tokio::spawn(async move { learn(&asking, LEARN_LIMIT).await });
        let command = sup.commands.recv().await;
        assert!(sup.apply(command).await);
        assert_eq!(
            heard_back(waiting).await,
            Err(NOT_CONNECTED_TO_LEARN.to_string())
        );

        drop(sup);
        assert_eq!(learn(&cmd_tx, LEARN_LIMIT).await, Err(GONE.to_string()));
    }

    /// A learn gives up after a minute. Once it has, a press is not taken:
    /// it fires as usual. Here the minute is a tenth of a second.
    #[tokio::test]
    async fn a_learn_gives_up_after_a_minute_and_the_press_then_fires() {
        assert_eq!(LEARN_LIMIT, Duration::from_secs(60));
        let (mut sup, cmd_tx, mut status_rx) = test_supervisor();
        connected(&mut sup);
        sup.actions = vec![shell_action("Say hi", "bb01", "exit 0")];
        let waiting = tokio::spawn(async move { learn(&cmd_tx, Duration::from_millis(100)).await });
        let command = sup.commands.recv().await;
        assert!(sup.apply(command).await);
        assert!(sup.learner.is_some(), "the band task holds the learn");

        assert_eq!(heard_back(waiting).await, Err(NO_PRESS.to_string()));
        sup.heard(message(ENDPOINT_APP, &[0xbb, 0x01])).await;
        let said = told(&mut status_rx);
        assert_eq!(
            pressed(&said[1]).action.as_deref(),
            Some("Say hi"),
            "{said:?}"
        );
        until_finished(&mut status_rx).await;
    }

    fn standing(state: Phase, reason: Reason, since: SystemTime) -> Standing {
        Standing {
            state,
            reason,
            since,
        }
    }

    /// A moment this many seconds ago.
    fn ago(secs: u64) -> SystemTime {
        SystemTime::now() - Duration::from_secs(secs)
    }
}

#[cfg(test)]
mod words_tests {
    use super::*;

    /// The messages a person reads that name this computer name the one
    /// they run on, this Mac or this PC, and say nothing that is only true
    /// on the other system.
    #[test]
    fn a_message_names_the_computer_it_runs_on() {
        #[cfg(target_os = "macos")]
        let (computer, other) = ("this Mac", "this PC");
        #[cfg(windows)]
        let (computer, other) = ("this PC", "this Mac");

        let nearby = BandError::NoBandNearby.to_string();
        assert_eq!(
            nearby,
            format!(
                "no band is nearby. Wake the band and keep it close to {computer}. If your \
                 phone is connected to it, turn off the phone's Bluetooth, then try again."
            )
        );
        assert_eq!(
            NO_KEY_TO_CONNECT,
            format!(
                "{computer} has no auth key for this band, so it cannot connect. Forget the \
                 band, then pair it again with the key from your account login."
            )
        );
        #[cfg(target_os = "macos")]
        let scan = "the scan did not start, so only a band this Mac holds can be found: off";
        #[cfg(windows)]
        let scan =
            "the scan did not start, so no band can be found. Turn Bluetooth on, then try again. off";
        assert_eq!(no_scan(&"off"), scan);

        let unavailable = BandError::BluetoothUnavailable.to_string();
        #[cfg(target_os = "macos")]
        assert!(
            unavailable.starts_with("bluetooth adapter did not become available within 20s"),
            "{unavailable}"
        );
        #[cfg(windows)]
        {
            assert_eq!(
                unavailable,
                "Bluetooth did not start within 20 seconds. Turn Bluetooth on, then try again."
            );
            assert!(!unavailable.contains("macOS"), "{unavailable}");
        }

        for text in [nearby.as_str(), NO_KEY_TO_CONNECT, scan, unavailable.as_str()] {
            assert!(!text.contains(other), "{text}");
        }
    }
}
