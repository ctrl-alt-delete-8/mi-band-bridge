//! What the band sends, and what the window's event log shows of it.
//!
//! [`BandEvent`] is every message the band sends, decoded. [`Record`] is a
//! line of the window's event log: a press, or an app installed or removed.
//! Only a press becomes a record of the band's messages. The rest is the
//! band's own traffic, its answers to our requests, and its battery reports,
//! which a terminal shows as they come and the window leaves out.

use crate::action::Outcome;
use crate::protocol::battery::{self, Battery};
use crate::protocol::chunk::ZeppMessage;
use crate::protocol::music::{ENDPOINT_MUSIC, MusicEvent, parse};
use serde::Serialize;
use std::time::SystemTime;

/// What the band told us, with no Zepp bytes above this layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BandEvent {
    Music(MusicEvent),
    /// What the band says about its battery, either because we asked or
    /// because the level changed.
    Battery(Battery),
    /// The band answering an app management request of ours. It arrives on the
    /// same endpoint as a press, and is not one.
    AppManagement(Vec<u8>),
    Unknown { endpoint: u16, payload: Vec<u8> },
}

/// The band's own service traffic, which is not a thing a person did.
///
/// The band asks the host to fetch weather and the like on this endpoint. Each
/// message is a few hundred bytes of URL and token, and showing them buries the
/// button presses.
pub const ENDPOINT_HTTP: u16 = 0x0001;

/// Signals sent by an app running on the band.
pub const ENDPOINT_APP: u16 = 0x00a0;

impl BandEvent {
    /// Whether this is something a person did, rather than the band's own
    /// housekeeping. Only these are worth showing by default.
    pub fn is_interesting(&self) -> bool {
        !matches!(
            self,
            BandEvent::Unknown {
                endpoint: ENDPOINT_HTTP,
                ..
            } | BandEvent::AppManagement(_)
                | BandEvent::Battery(_)
        )
    }

    /// The signal this is, when it is a press: bytes a band app sent on the
    /// app endpoint that follow the signal rule, in the lower case hex an
    /// action names them by.
    ///
    /// Nothing else carries one: not the band's own traffic, not its answers
    /// to our requests, not its battery reports, and not the stock music
    /// screen. So only a press fires an action, or reaches the event log.
    pub fn signal(&self) -> Option<String> {
        match self {
            BandEvent::Unknown {
                endpoint: ENDPOINT_APP,
                payload,
            } => crate::signal::from_bytes(payload),
            _ => None,
        }
    }

    pub fn from_message(message: ZeppMessage) -> Self {
        if message.endpoint == ENDPOINT_MUSIC
            && let Some(event) = parse(&message.payload)
        {
            return BandEvent::Music(event);
        }
        // App management shares the endpoint a band app sends on, and is told
        // apart by its first byte. Without this, the band's answer to a list
        // request would read as somebody pressing a button.
        if message.endpoint == battery::ENDPOINT
            && let Some(state) = battery::parse_reply(&message.payload)
        {
            return BandEvent::Battery(state);
        }
        if message.endpoint == ENDPOINT_APP
            && crate::protocol::apps::is_management(&message.payload)
        {
            return BandEvent::AppManagement(message.payload);
        }
        BandEvent::Unknown {
            endpoint: message.endpoint,
            payload: message.payload,
        }
    }
}

impl std::fmt::Display for BandEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BandEvent::Music(event) => write!(f, "[music] {}", event.label()),
            BandEvent::Battery(state) => write!(f, "[battery] {state}"),
            BandEvent::AppManagement(payload) => write!(f, "[apps] {}", hex::encode(payload)),
            // A signal from an app on the band is the ordinary case, not an
            // oddity, so it reads as one. Anything else keeps its endpoint,
            // because the endpoint is the useful part when it is a surprise.
            BandEvent::Unknown {
                endpoint: ENDPOINT_APP,
                payload,
            } => write!(f, "[app] {}", hex::encode(payload)),
            BandEvent::Unknown { endpoint, payload } => write!(
                f,
                "[unknown] endpoint={:#06x} payload={}",
                endpoint,
                hex::encode(payload)
            ),
        }
    }
}

/// One line of the window's event log: a press, or an app installed or
/// removed. Nothing else becomes one: not the band's own traffic, not its
/// answers to our requests, not a battery report, not a change to the menu,
/// and not a change to the connection, which the connection card shows.
///
/// The page gets it as JSON, `kind` first, then the fields of the kind:
///
/// ```text
/// {"kind":"press","id":7,"at":"2026-09-24T21:04:11-04:00","signal":"bb04",
///  "action":"Turn on the desk lights","result":{"type":"status","ok":true,"code":204}}
/// {"kind":"app","id":8,"at":"2026-09-24T21:02:58-04:00","change":"installed","app":"testbandapp"}
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Record {
    Press(Press),
    App(AppChange),
}

/// A press: the signal a band app sent, the action it fired, and how that
/// went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Press {
    /// This record's number, which no other record in this run shares. A
    /// press whose action takes time is sent again under the same number,
    /// with its result, so the page writes it over its own line.
    pub id: u64,
    /// When the press came, in local time, as RFC 3339 to the second.
    #[serde(serialize_with = "crate::supervisor::as_local_time")]
    pub at: SystemTime,
    /// The signal, in lower case hex: `bb04`.
    pub signal: String,
    /// The name of the action the press fired. None when no action answers
    /// the signal: the press is still a press.
    pub action: Option<String>,
    /// What the action does, in a few words, for a terminal's `fired` line.
    /// The event log shows the name alone, so the page is not sent it.
    #[serde(skip)]
    pub does: Option<String>,
    /// How the action went. None while it runs, and when there is no action.
    pub result: Option<Outcome>,
}

/// An app installed on the band, or removed from it, by any caller: the
/// window, or the band app editor through the socket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppChange {
    /// This record's number, shared with no other record in this run.
    pub id: u64,
    /// When the install or the removal finished, in local time.
    #[serde(serialize_with = "crate::supervisor::as_local_time")]
    pub at: SystemTime,
    pub change: Change,
    /// The app's name, from its manifest. An app with no name known goes by
    /// its id when removed, and by its package's file name when installed.
    pub app: String,
}

/// What happened to an app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Installed,
    Removed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::apps::{CMD_APPS, FROM_BAND};
    use std::time::{Duration, UNIX_EPOCH};

    /// A message on this endpoint with these bytes, decoded.
    fn heard(endpoint: u16, payload: &[u8]) -> BandEvent {
        BandEvent::from_message(ZeppMessage {
            endpoint,
            payload: payload.to_vec(),
            encrypted: false,
        })
    }

    /// Bytes a band app sends by the signal rule are a press, whatever case
    /// an action writes them in.
    #[test]
    fn a_signal_from_a_band_app_is_a_press() {
        assert_eq!(
            heard(ENDPOINT_APP, &[0xbb, 0x01]).signal().as_deref(),
            Some("bb01")
        );
        assert_eq!(
            heard(ENDPOINT_APP, &[0xbb, 0x1f]).signal().as_deref(),
            Some("bb1f")
        );
        assert_eq!(
            heard(ENDPOINT_APP, &[0xbb, 0x04, 0x01]).signal().as_deref(),
            Some("bb0401")
        );
    }

    /// Everything else the band sends carries no signal, so none of it can
    /// fire an action or reach the event log.
    #[test]
    fn nothing_but_a_press_carries_a_signal() {
        let mut list_reply = vec![CMD_APPS, FROM_BAND, 0x01];
        list_reply.resize(24, 0);
        let not_presses = [
            // The band's own traffic: weather and the like.
            heard(ENDPOINT_HTTP, &[0xbb, 0x01]),
            // An answer to our app list request.
            heard(ENDPOINT_APP, &list_reply),
            // The band's platform messages on the app endpoint: JavaScript,
            // and a screenshot.
            heard(ENDPOINT_APP, &[0x01, 0xbb, 0x01]),
            heard(ENDPOINT_APP, &[0x03, 0x00]),
            // Too short and too long to be a signal.
            heard(ENDPOINT_APP, &[0xbb]),
            heard(ENDPOINT_APP, &[0xbb, 1, 2, 3, 4, 5, 6, 7, 8]),
            // The right bytes on another endpoint.
            heard(0x0010, &[0xbb, 0x01]),
            // The stock music screen.
            heard(ENDPOINT_MUSIC, &[0x05, 0x05]),
            // A battery report.
            BandEvent::Battery(Battery {
                percent: 74,
                charging: false,
            }),
        ];
        assert!(matches!(not_presses[1], BandEvent::AppManagement(_)));
        assert!(matches!(not_presses[7], BandEvent::Music(_)));
        for event in not_presses {
            assert_eq!(event.signal(), None, "{event}");
        }
    }

    /// A moment `secs` seconds into the test's own clock.
    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_790_298_000 + secs)
    }

    fn press(action: Option<&str>, result: Option<Outcome>) -> Record {
        Record::Press(Press {
            id: 7,
            at: at(11),
            signal: "bb04".to_string(),
            action: action.map(str::to_string),
            does: action.map(|_| "POST http://lights.local/webhook".to_string()),
            result,
        })
    }

    /// The record as the page reads it. `at` is local time, which is checked
    /// once, by the test below, and left out of the others.
    fn wire(record: &Record) -> serde_json::Value {
        let mut json = serde_json::to_value(record).unwrap();
        let at = json
            .as_object_mut()
            .unwrap()
            .remove("at")
            .expect("every record has a time");
        assert!(at.is_string(), "{at}");
        json
    }

    /// A press with its action and its result, and what the page does not
    /// get: what the action does, which is for a terminal.
    #[test]
    fn a_press_is_its_time_signal_action_and_result() {
        let record = press(Some("Turn on the desk lights"), Some(Outcome::Status(204)));
        let json = serde_json::to_value(&record).unwrap();
        let read = chrono::DateTime::parse_from_rfc3339(json["at"].as_str().unwrap()).unwrap();
        assert_eq!(
            read.timestamp(),
            at(11).duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
        );
        assert_eq!(
            json["at"].as_str().unwrap(),
            crate::supervisor::local_time(at(11)),
            "local time, to the second, with its offset"
        );
        assert_eq!(
            wire(&record),
            serde_json::json!({
                "kind": "press",
                "id": 7,
                "signal": "bb04",
                "action": "Turn on the desk lights",
                "result": {"type": "status", "ok": true, "code": 204},
            })
        );
        assert!(json.get("does").is_none(), "{json}");
        let text = serde_json::to_string(&record).unwrap();
        assert!(
            text.starts_with(r#"{"kind":"press","id":7,"at":""#),
            "kind comes first: {text}"
        );
    }

    /// A press whose action still runs has no result yet. A press whose
    /// signal no action answers has neither, and is still a press.
    #[test]
    fn a_press_that_runs_or_fired_nothing_has_no_result() {
        let running = wire(&press(Some("Turn on the desk lights"), None));
        assert_eq!(running["action"], "Turn on the desk lights");
        assert!(running["result"].is_null(), "{running}");

        let nothing = wire(&press(None, None));
        assert_eq!(
            nothing,
            serde_json::json!({
                "kind": "press",
                "id": 7,
                "signal": "bb04",
                "action": null,
                "result": null,
            })
        );
    }

    /// An app installed or removed, by its name.
    #[test]
    fn an_app_is_installed_or_removed_by_name() {
        for (change, word) in [
            (Change::Installed, "installed"),
            (Change::Removed, "removed"),
        ] {
            let record = Record::App(AppChange {
                id: 8,
                at: at(3),
                change,
                app: "testbandapp".to_string(),
            });
            assert_eq!(
                wire(&record),
                serde_json::json!({"kind": "app", "id": 8, "change": word, "app": "testbandapp"})
            );
        }
    }

    /// A press from a band app is the ordinary case and reads as one.
    #[test]
    fn an_app_signal_reads_plainly() {
        let event = BandEvent::Unknown {
            endpoint: ENDPOINT_APP,
            payload: vec![0xbb, 0x01],
        };
        assert_eq!(event.to_string(), "[app] bb01");
        assert!(event.is_interesting());
    }

    /// The band's own service traffic keeps its endpoint, and stays out of the
    /// way by default.
    #[test]
    fn housekeeping_is_not_interesting() {
        let event = BandEvent::Unknown {
            endpoint: ENDPOINT_HTTP,
            payload: vec![0x01, 0x02],
        };
        assert!(!event.is_interesting());
        assert!(event.to_string().contains("0x0001"));
    }

    #[test]
    fn maps_a_music_message() {
        let msg = ZeppMessage {
            endpoint: ENDPOINT_MUSIC,
            payload: vec![0x05, 0x05],
            encrypted: false,
        };
        let event = BandEvent::from_message(msg);
        assert_eq!(event, BandEvent::Music(MusicEvent::VolumeUp));
        assert_eq!(event.to_string(), "[music] volume_up");
    }

    #[test]
    fn keeps_an_unparsed_music_payload_as_unknown() {
        let msg = ZeppMessage {
            endpoint: ENDPOINT_MUSIC,
            payload: vec![0x05, 0x02],
            encrypted: false,
        };
        let event = BandEvent::from_message(msg);
        assert_eq!(
            event,
            BandEvent::Unknown {
                endpoint: ENDPOINT_MUSIC,
                payload: vec![0x05, 0x02],
            }
        );
    }

    #[test]
    fn formats_an_unknown_endpoint() {
        let msg = ZeppMessage {
            endpoint: 0x0010,
            payload: vec![0xaa, 0xbb],
            encrypted: false,
        };
        let event = BandEvent::from_message(msg);
        assert_eq!(event.to_string(), "[unknown] endpoint=0x0010 payload=aabb");
    }
}
