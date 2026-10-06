//! Matching a press to the action it should run.

use crate::config::{Action, Does};

/// A short description of what an action does, for showing a person.
pub fn describe(does: &Does) -> String {
    match does {
        Does::Keystroke(keystroke) => {
            if let Some(media) = &keystroke.consumer_key_code {
                return media.clone();
            }
            let key = keystroke.key_code.clone().unwrap_or_default();
            if keystroke.modifiers.is_empty() {
                key
            } else {
                format!("{} {key}", keystroke.modifiers.join(" "))
            }
        }
        Does::Shell(line) => format!("shell: {line}"),
        Does::Script(path) => format!("script: {path}"),
        Does::Shortcut(name) => format!("shortcut: {name}"),
        Does::WebRequest(request) => format!("{} {}", request.method(), request.url),
    }
}

/// Does any of these post a key event?
///
/// Only a keystroke needs the input permission. A config of shell commands,
/// scripts, shortcuts, and web requests must start on a Mac that has granted
/// nothing.
pub fn any_posts_keys(actions: &[Action]) -> bool {
    actions
        .iter()
        .any(|action| matches!(action.does, Does::Keystroke(_)))
}

/// Every action that answers a press's signal, in the order of the list.
///
/// Several actions may share a signal (D6). A press runs them one by one,
/// in this order, and the person decides how they fit together.
///
/// The signal is the one [`crate::event::BandEvent::signal`] gives, in the
/// lower case hex the band sends, so `BB01` in the file answers the bytes
/// bb 01. Only a press has a signal: the band's own traffic, its answers to
/// our requests, its battery reports and the stock music screen fire nothing.
pub fn matching<'a>(actions: &'a [Action], signal: &str) -> Vec<&'a Action> {
    actions
        .iter()
        .filter(|action| crate::signal::normalize(&action.signal).is_ok_and(|own| own == signal))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Keystroke, WebRequest};
    use crate::event::BandEvent;
    use crate::protocol::chunk::ZeppMessage;

    fn press(payload: &[u8]) -> BandEvent {
        heard(0x00a0, payload)
    }

    fn heard(endpoint: u16, payload: &[u8]) -> BandEvent {
        BandEvent::from_message(ZeppMessage {
            endpoint,
            payload: payload.to_vec(),
            encrypted: false,
        })
    }

    /// The first action this band event fires, as the supervisor finds them.
    fn match_event<'a>(actions: &'a [Action], event: &BandEvent) -> Option<&'a Action> {
        matching(actions, &event.signal()?).into_iter().next()
    }

    fn action(signal: &str, does: Does) -> Action {
        Action {
            name: format!("the {signal} action"),
            signal: signal.to_string(),
            does,
        }
    }

    fn key(signal: &str, key: &str, modifiers: &[&str]) -> Action {
        action(
            signal,
            Does::Keystroke(Keystroke {
                key_code: Some(key.to_string()),
                consumer_key_code: None,
                modifiers: modifiers.iter().map(|m| m.to_string()).collect(),
            }),
        )
    }

    #[test]
    fn matches_a_signal_by_payload() {
        let actions = vec![key("bb01", "a", &["command"])];
        let hit = match_event(&actions, &press(&[0xbb, 0x01])).expect("bb01 is bound");
        assert_eq!(hit.name, "the bb01 action");
    }

    /// Every action on a signal answers a press, in the order of the list,
    /// whatever case its signal is written in (D6).
    #[test]
    fn every_action_on_a_signal_answers_in_list_order() {
        let named = |name: &str, signal: &str| Action {
            name: name.to_string(),
            ..action(signal, Does::Shell("true".to_string()))
        };
        let actions = vec![
            named("First", "bb01"),
            named("Elsewhere", "bb02"),
            named("Second", "BB01"),
        ];
        let names: Vec<&str> = matching(&actions, "bb01")
            .iter()
            .map(|action| action.name.as_str())
            .collect();
        assert_eq!(names, ["First", "Second"]);
        assert!(matching(&actions, "bb09").is_empty());
    }

    #[test]
    fn a_payload_that_differs_by_one_byte_does_not_match() {
        let actions = vec![key("bb01", "a", &[])];
        assert!(match_event(&actions, &press(&[0xbb, 0x02])).is_none());
        assert!(match_event(&actions, &press(&[0xbb])).is_none());
        assert!(match_event(&actions, &press(&[0xbb, 0x01, 0x00])).is_none());
    }

    /// The editor accepts a signal in either case, so the file may hold one.
    #[test]
    fn a_signal_written_in_upper_case_still_matches() {
        let actions = vec![key("BB1F", "a", &[])];
        assert!(match_event(&actions, &press(&[0xbb, 0x1f])).is_some());
    }

    /// `music:` signals are gone: the stock Music screen fires no action.
    #[test]
    fn the_music_screen_fires_nothing() {
        let actions = vec![key("bb05", "a", &[])];
        let volume_up = BandEvent::from_message(ZeppMessage {
            endpoint: crate::protocol::music::ENDPOINT_MUSIC,
            payload: vec![0x05, 0x05],
            encrypted: false,
        });
        assert!(matches!(volume_up, BandEvent::Music(_)));
        assert!(match_event(&actions, &volume_up).is_none());
    }

    /// A press comes from a band app, on the app endpoint. The same bytes on
    /// another endpoint are the band's own traffic, and fire nothing.
    #[test]
    fn the_same_bytes_on_another_endpoint_fire_nothing() {
        let actions = vec![key("bb01", "a", &[])];
        assert!(match_event(&actions, &press(&[0xbb, 0x01])).is_some());
        for endpoint in [crate::event::ENDPOINT_HTTP, 0x0010] {
            assert!(
                match_event(&actions, &heard(endpoint, &[0xbb, 0x01])).is_none(),
                "{endpoint:#06x}"
            );
        }
    }

    #[test]
    fn only_a_keystroke_needs_the_input_permission() {
        let keys = vec![key("bb01", "a", &[])];
        assert!(any_posts_keys(&keys));

        let others = vec![
            action("bb02", Does::Shell("true".to_string())),
            action("bb03", Does::Shortcut("Focus".to_string())),
            action("bb04", Does::Script("~/lamp.sh".to_string())),
            action(
                "bb05",
                Does::WebRequest(WebRequest {
                    url: "https://example.com/hook".to_string(),
                    ..WebRequest::default()
                }),
            ),
        ];
        assert!(!any_posts_keys(&others), "none of these posts a key");
        assert!(any_posts_keys(&[others, keys].concat()), "one is enough");
    }

    #[test]
    fn describes_every_kind_in_words_a_person_reads() {
        assert_eq!(describe(&key("bb04", "a", &["command"]).does), "command a");
        let media = Does::Keystroke(Keystroke {
            consumer_key_code: Some("play_or_pause".to_string()),
            ..Keystroke::default()
        });
        assert_eq!(describe(&media), "play_or_pause");
        assert_eq!(describe(&Does::Shell("open -a Music".to_string())), "shell: open -a Music");
        assert_eq!(describe(&Does::Script("~/lamp.sh".to_string())), "script: ~/lamp.sh");
        assert_eq!(describe(&Does::Shortcut("Start Focus".to_string())), "shortcut: Start Focus");
        let call = Does::WebRequest(WebRequest {
            url: "https://example.com/hook".to_string(),
            body: Some("{}".to_string()),
            ..WebRequest::default()
        });
        assert_eq!(describe(&call), "POST https://example.com/hook");
    }
}
