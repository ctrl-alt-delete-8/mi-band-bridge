//! What a signal is.
//!
//! A band app sends a few bytes on the app endpoint, and an action names them
//! in hex. This is the rule the band app editor enforces on its buttons,
//! written once here because this library cannot depend on the editor, which
//! brings a window toolkit with it. Every signal the editor accepts is accepted
//! here too, so every button it makes can be given an action.

/// The first byte of every signal.
///
/// The band's own platform messages share the endpoint an app sends on, and
/// they are told apart by the first byte: 0x01 is JavaScript, 0x02 is app
/// management, 0x03 is a screenshot. One prefix for every app keeps well clear
/// of them.
pub const PREFIX: u8 = 0xbb;

/// What a signal is, as a refusal says it: the prefix, then one to seven
/// bytes, each a pair of hex digits.
const WHAT_A_SIGNAL_IS: &str =
    "A signal is bb followed by one to seven pairs of hex digits, such as bb01 or bb0401.";

/// The signal as the band sends it, in lower case hex, or why the text is not
/// a signal.
///
/// Upper case and surrounding spaces are accepted, as the editor accepts them,
/// so `BB1F` and `bb1f` are one signal.
pub fn normalize(text: &str) -> Result<String, String> {
    match parse(text) {
        Some(bytes) if follows_the_rule(&bytes) => Ok(hex::encode(bytes)),
        _ => Err(format!("\"{text}\" is not a signal. {WHAT_A_SIGNAL_IS}")),
    }
}

/// The signal these bytes are, in lower case hex, when they follow the rule.
/// This is how a press is told apart from everything else a band app's
/// endpoint carries.
pub fn from_bytes(bytes: &[u8]) -> Option<String> {
    follows_the_rule(bytes).then(|| hex::encode(bytes))
}

/// The prefix, then one to seven bytes.
fn follows_the_rule(bytes: &[u8]) -> bool {
    bytes.first() == Some(&PREFIX) && (2..=8).contains(&bytes.len())
}

/// Hex text to bytes: one to eight bytes, upper or lower case.
fn parse(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.is_empty() || !text.len().is_multiple_of(2) || text.len() > 16 {
        return None;
    }
    hex::decode(text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a signal is, as the refusal says it.
    const RULE: &str =
        "A signal is bb followed by one to seven pairs of hex digits, such as bb01 or bb0401.";

    /// The same cases the editor's own test uses, so the two rules cannot
    /// drift apart without one of them failing.
    #[test]
    fn accepts_what_the_editor_accepts() {
        for (good, sent) in [("bb04", "bb04"), ("BB1F", "bb1f"), ("bb0401", "bb0401"), (" bb01 ", "bb01")] {
            assert_eq!(normalize(good).as_deref(), Ok(sent), "{good}");
        }
    }

    #[test]
    fn refuses_what_the_editor_refuses() {
        for bad in ["0201", "02", "c0ffee", "bb", "ff04"] {
            assert_eq!(
                normalize(bad).unwrap_err(),
                format!("\"{bad}\" is not a signal. {RULE}"),
                "{bad}"
            );
        }
    }

    /// Text that is not hex, or not whole pairs of digits, or too long, gets
    /// the same words: what a signal is.
    #[test]
    fn says_what_a_signal_is() {
        for bad in ["bb0g", "music:volume_up", "bb0", "bb0102030405060708"] {
            assert_eq!(
                normalize(bad).unwrap_err(),
                format!("\"{bad}\" is not a signal. {RULE}"),
                "{bad}"
            );
        }
        // Seven pairs after bb is the most a signal holds.
        assert_eq!(
            normalize("bb01020304050607").as_deref(),
            Ok("bb01020304050607")
        );
    }

    /// The bytes a band app sends follow the same rule as the text an
    /// action names them by, so every signal an action can name is a press.
    #[test]
    fn bytes_are_a_signal_by_the_same_rule() {
        for (bytes, signal) in [
            (&[0xbb, 0x04][..], "bb04"),
            (&[0xbb, 0x1f][..], "bb1f"),
            (&[0xbb, 0x04, 0x01][..], "bb0401"),
            (&[0xbb, 1, 2, 3, 4, 5, 6, 7][..], "bb01020304050607"),
        ] {
            assert_eq!(from_bytes(bytes).as_deref(), Some(signal));
            assert_eq!(normalize(signal).as_deref(), Ok(signal), "{signal}");
        }
        for bytes in [
            &[][..],
            &[0xbb][..],
            &[0x02, 0x00, 0x01][..],
            &[0x01, 0xbb][..],
            &[0xbb, 1, 2, 3, 4, 5, 6, 7, 8][..],
        ] {
            assert_eq!(from_bytes(bytes), None, "{bytes:02x?}");
        }
    }

    #[test]
    fn refuses_text_that_is_not_hex() {
        for bad in ["", "bb0", "zz", "bb0g", "music:volume_up", "bb0102030405060708"] {
            let why = normalize(bad).unwrap_err();
            assert!(why.contains("is not a signal"), "{bad}: {why}");
        }
    }
}
