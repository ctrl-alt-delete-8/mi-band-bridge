//! Turning Karabiner-style names into key events.
//!
//! Each kind of name has one table here: the keys, the media keys, and the
//! modifiers. The loader checks a name against its table, a keystroke is
//! posted from it, and the window lists it, so every name the window offers
//! is one the file takes.
//!
//! The names are Karabiner's on macOS and on Windows alike (W2), and the
//! numbers in the tables are macOS's. [`crate::platform::keys`] posts the
//! keys, and on Windows holds the virtual key each name stands for, the
//! names Windows cannot send, each with why, and the keys a PC keyboard has
//! beyond a Mac's, such as insert. A name this system cannot send is refused
//! by the loader and left out of the window's list.

use serde::Serialize;
use std::collections::BTreeMap;

/// CGEventFlags bits. Named here so the tables stay plain integers.
pub const FLAG_SHIFT: u64 = 0x0002_0000;
pub const FLAG_CONTROL: u64 = 0x0004_0000;
pub const FLAG_OPTION: u64 = 0x0008_0000;
pub const FLAG_COMMAND: u64 = 0x0010_0000;
pub const FLAG_FN: u64 = 0x0080_0000;

/// Every key a keystroke can name in `key_code`, by the name Karabiner gives
/// it, with the macOS virtual keycode it posts. In the order the window lists
/// them: letters, digits, the keys around them, arrows and the navigation
/// cluster, function keys, the keypad, the modifier keys, and the keys of
/// Japanese keyboards.
///
/// The numbers are the ANSI layout constants from Carbon's Events.h. They are
/// positional, not letters: keycode 0 is the key where a US layout has "a".
///
/// Every name Karabiner-Elements offers is here, in [`KEY_ALIASES`], or in
/// [`unsupported_key`], so a name read off Karabiner-EventViewer either works
/// or says why it cannot. A test checks that against Karabiner's own list.
#[rustfmt::skip]
pub(crate) const KEYS: &[(&str, u16)] = &[
    ("a", 0), ("b", 11), ("c", 8), ("d", 2), ("e", 14), ("f", 3), ("g", 5),
    ("h", 4), ("i", 34), ("j", 38), ("k", 40), ("l", 37), ("m", 46), ("n", 45),
    ("o", 31), ("p", 35), ("q", 12), ("r", 15), ("s", 1), ("t", 17), ("u", 32),
    ("v", 9), ("w", 13), ("x", 7), ("y", 16), ("z", 6),
    ("0", 29), ("1", 18), ("2", 19), ("3", 20), ("4", 21), ("5", 23), ("6", 22),
    ("7", 26), ("8", 28), ("9", 25),
    ("return_or_enter", 36), ("tab", 48), ("spacebar", 49),
    ("delete_or_backspace", 51), ("escape", 53),
    ("hyphen", 27), ("equal_sign", 24), ("open_bracket", 33),
    ("close_bracket", 30), ("backslash", 42), ("semicolon", 41), ("quote", 39),
    ("grave_accent_and_tilde", 50), ("comma", 43), ("period", 47), ("slash", 44),
    // The key an ISO keyboard has beside the left shift.
    ("non_us_backslash", 10),
    ("up_arrow", 126), ("down_arrow", 125), ("left_arrow", 123),
    ("right_arrow", 124),
    // The navigation cluster. A keyboard without one still sends these
    // through fn, and an app reads them the same either way.
    ("home", 115), ("end", 119), ("page_up", 116), ("page_down", 121),
    ("delete_forward", 117),
    // The Mac key in the place a PC keyboard puts insert.
    ("help", 114),
    ("f1", 122), ("f2", 120), ("f3", 99), ("f4", 118), ("f5", 96), ("f6", 97),
    ("f7", 98), ("f8", 100), ("f9", 101), ("f10", 109), ("f11", 103),
    ("f12", 111),
    // A keyboard rarely has these, which is what makes them useful for a band
    // button: nothing else on the machine sends them. Karabiner names them the
    // same way.
    ("f13", 105), ("f14", 107), ("f15", 113), ("f16", 106), ("f17", 64),
    ("f18", 79), ("f19", 80), ("f20", 90),
    ("keypad_0", 82), ("keypad_1", 83), ("keypad_2", 84), ("keypad_3", 85),
    ("keypad_4", 86), ("keypad_5", 87), ("keypad_6", 88), ("keypad_7", 89),
    ("keypad_8", 91), ("keypad_9", 92),
    ("keypad_period", 65), ("keypad_enter", 76), ("keypad_plus", 69),
    ("keypad_hyphen", 78), ("keypad_asterisk", 67), ("keypad_slash", 75),
    ("keypad_equal_sign", 81), ("keypad_comma", 95),
    // A Mac keypad has clear where a PC keypad has num lock.
    ("keypad_num_lock", 71),
    // A modifier is a key in its own right, and Karabiner names it one. A side
    // is kept here, because macOS has a keycode for each.
    ("left_control", 59), ("left_option", 58), ("left_shift", 56),
    ("left_command", 55), ("right_control", 62), ("right_option", 61),
    ("right_shift", 60), ("right_command", 54), ("caps_lock", 57), ("fn", 63),
    // Japanese keyboards.
    ("japanese_eisuu", 102), ("japanese_kana", 104), ("international1", 94),
    ("international3", 93),
];

/// Karabiner's other names for some of these keys, each with the name
/// [`KEYS`] lists the key under.
const KEY_ALIASES: &[(&str, &str)] = &[
    // Karabiner's older spellings.
    ("return", "return_or_enter"),
    ("prior", "page_up"),
    // The names a PC keyboard gives the same keys.
    ("left_gui", "left_command"),
    ("right_gui", "right_command"),
    ("left_alt", "left_option"),
    ("right_alt", "right_option"),
    // The Mac keypad's name for the key a PC keypad calls num lock.
    ("clear", "keypad_num_lock"),
    // The same two Japanese keys, under Karabiner's other names for them.
    ("lang1", "japanese_kana"),
    ("lang2", "japanese_eisuu"),
];

/// Every media key a keystroke can name in `consumer_key_code`, by the name
/// Karabiner gives it, with the NX_KEYTYPE constant it posts, in the order the
/// window lists them.
///
/// Karabiner names far more consumer keys than macOS can send. These are the
/// ones with an NX_KEYTYPE constant behind them.
#[rustfmt::skip]
pub(crate) const MEDIA_KEYS: &[(&str, u8)] = &[
    ("volume_increment", 0), ("volume_decrement", 1), ("mute", 7),
    ("play_or_pause", 16), ("scan_next_track", 17), ("scan_previous_track", 18),
    ("fast_forward", 19), ("rewind", 20), ("eject", 14),
    ("display_brightness_increment", 2), ("display_brightness_decrement", 3),
    ("illumination_increment", 21), ("illumination_decrement", 22),
];

/// Karabiner spells one media key both ways, in two of its own lists.
const MEDIA_KEY_ALIASES: &[(&str, &str)] = &[("fastforward", "fast_forward")];

/// Every modifier a keystroke can hold, by the name Karabiner gives it, with
/// its CGEventFlags bit, in the order the window lists them.
pub(crate) const MODIFIERS: &[(&str, u64)] = &[
    ("command", FLAG_COMMAND),
    ("shift", FLAG_SHIFT),
    ("option", FLAG_OPTION),
    ("control", FLAG_CONTROL),
    ("fn", FLAG_FN),
];

/// Shorter names for three of the modifiers.
const MODIFIER_ALIASES: &[(&str, &str)] =
    &[("cmd", "command"), ("alt", "option"), ("ctrl", "control")];

/// Karabiner names a modifier with a side, such as left_command. macOS posts
/// a modifier without one, so a side is accepted and then ignored: refusing a
/// name a person reasonably copied from Karabiner helps nobody.
const SIDES: [&str; 2] = ["left_", "right_"];

/// Karabiner modifier name to a CGEventFlags bit. See [`MODIFIERS`].
///
/// On Windows the bit only names the modifier. The platform module holds the
/// key it stands for there.
pub fn modifier_flag(name: &str) -> Option<u64> {
    lookup(MODIFIERS, listed_modifier(name)?)
}

/// Karabiner key name to a macOS virtual keycode. See [`KEYS`].
pub fn key_code(name: &str) -> Option<u16> {
    lookup(KEYS, listed_key(name)?)
}

/// Karabiner consumer key name to an NX_KEYTYPE constant. See [`MEDIA_KEYS`].
pub fn consumer_code(name: &str) -> Option<u8> {
    lookup(MEDIA_KEYS, listed_media_key(name)?)
}

/// The name [`KEYS`] lists a key under, whichever spelling the file uses, or
/// None for a name it does not list.
pub(crate) fn listed_key(name: &str) -> Option<&'static str> {
    listed_in(KEYS, listed(KEY_ALIASES, name))
}

/// The name [`MEDIA_KEYS`] lists a media key under, whichever spelling the
/// file uses, or None for a name it does not list.
pub(crate) fn listed_media_key(name: &str) -> Option<&'static str> {
    listed_in(MEDIA_KEYS, listed(MEDIA_KEY_ALIASES, name))
}

/// The name [`MODIFIERS`] lists a modifier under, with a side, by a shorter
/// name, or both, or None for a name it does not list.
pub(crate) fn listed_modifier(name: &str) -> Option<&'static str> {
    let bare = SIDES
        .iter()
        .find_map(|side| name.strip_prefix(side))
        .unwrap_or(name);
    listed_in(MODIFIERS, listed(MODIFIER_ALIASES, bare))
}

/// Whether this system sends a key the file names: a name [`KEYS`] lists,
/// in any spelling, that the platform does not refuse, or a key only this
/// system has, such as insert on Windows. The Mac sends every key [`KEYS`]
/// lists, and no other. See [`unsupported_key`] and [`keys_only_here`].
pub fn sends_key(name: &str) -> bool {
    (key_code(name).is_some() && unsupported_key(name).is_none())
        || keys_only_here().any(|own| own == name)
}

/// Whether this system sends a media key the file names. See
/// [`unsupported_media_key`].
pub fn sends_media_key(name: &str) -> bool {
    consumer_code(name).is_some() && unsupported_media_key(name).is_none()
}

/// Whether this system holds a modifier the file names. See
/// [`unsupported_modifier`].
pub fn sends_modifier(name: &str) -> bool {
    modifier_flag(name).is_some() && unsupported_modifier(name).is_none()
}

/// Posting keys, naming the keys the window's Record keys hears, and saying
/// why a name cannot be sent: each system's own, behind the same names (W1).
pub use crate::platform::keys::{
    LABELS, fire, heard_key, heard_media_key, key_name, keys_only_here, media_key_name, tap_key,
    tap_media, trusted, unsupported_key, unsupported_media_key, unsupported_modifier,
};

// The mark on every key event the window sends on Windows, and the blank-key
// tap, so the window's Record keys can tell its own keys from a person's.
#[cfg(windows)]
pub use crate::platform::keys::{SENT_BY_BANDBRIDGE, tap_blank};

/// The name a table lists `name` under: the name it is another spelling of,
/// or the name itself.
fn listed<'a>(aliases: &[(&str, &'a str)], name: &'a str) -> &'a str {
    aliases
        .iter()
        .find(|alias| alias.0 == name)
        .map_or(name, |alias| alias.1)
}

/// The table's own copy of a name it lists, which lives as long as the
/// program.
fn listed_in<T>(table: &[(&'static str, T)], name: &str) -> Option<&'static str> {
    table.iter().find(|entry| entry.0 == name).map(|entry| entry.0)
}

/// What a table holds for a name.
fn lookup<T: Copy>(table: &[(&str, T)], name: &str) -> Option<T> {
    table
        .iter()
        .find(|entry| entry.0 == name)
        .map(|entry| entry.1)
}

/// The names in a table, in its order.
fn names_in<T>(table: &[(&'static str, T)]) -> Vec<&'static str> {
    table.iter().map(|entry| entry.0).collect()
}

/// Every modifier name this program accepts, for an error message.
pub fn modifier_names() -> Vec<&'static str> {
    sent(MODIFIERS, sends_modifier)
}

/// The names in a table this system sends, in the table's order.
fn sent<T>(table: &[(&'static str, T)], sends: fn(&str) -> bool) -> Vec<&'static str> {
    names_in(table).into_iter().filter(|name| sends(name)).collect()
}

/// Whether a key is a modifier in its own right, such as left_command or
/// caps_lock. Record keys waits past these for the key they go with.
pub fn is_modifier_key(key: &str) -> bool {
    matches!(
        key,
        "left_control"
            | "left_option"
            | "left_shift"
            | "left_command"
            | "right_control"
            | "right_option"
            | "right_shift"
            | "right_command"
            | "caps_lock"
            | "fn"
    )
}

/// Every media key name this program can send, for an error message.
pub fn media_key_names() -> Vec<&'static str> {
    sent(MEDIA_KEYS, sends_media_key)
}

/// The names a keystroke can use, for the window's list of keys (D3): each
/// key once, by the name Karabiner gives it now, and the other spellings a
/// file may hold. See [`names`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyNames {
    /// What `key_code` takes.
    pub keys: Vec<&'static str>,
    /// What `consumer_key_code` takes: the media keys.
    pub media_keys: Vec<&'static str>,
    /// What `modifiers` takes.
    pub modifiers: Vec<&'static str>,
    /// The other spellings each group takes, each with the listed name it
    /// stands for.
    pub aliases: Aliases,
    /// The label the window shows for each modifier, on a system whose keys
    /// are not the Mac's: Ctrl, Alt, Shift and Win on Windows (W2). The Mac's
    /// window draws its own marks, so on the Mac this is empty and left out.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<&'static str, &'static str>,
}

/// The other spellings of each group, each with the listed name it stands
/// for: `return` is `return_or_enter` among the keys, and `left_command` is
/// `command` among the modifiers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Aliases {
    pub keys: BTreeMap<String, &'static str>,
    pub media_keys: BTreeMap<String, &'static str>,
    pub modifiers: BTreeMap<String, &'static str>,
}

/// Every name the file takes for a key, a media key and a modifier: the
/// window's list of keys, which is there whether or not the window can hear
/// a key (D3). A name this system cannot send is left out, and so is every
/// other spelling of it.
pub fn names() -> KeyNames {
    let aliases_in = |aliases: &[(&str, &'static str)], sends: fn(&str) -> bool| {
        aliases
            .iter()
            .filter(|alias| sends(alias.1))
            .map(|alias| (alias.0.to_string(), alias.1))
            .collect()
    };
    KeyNames {
        keys: sent(KEYS, sends_key)
            .into_iter()
            .chain(keys_only_here())
            .collect(),
        media_keys: sent(MEDIA_KEYS, sends_media_key),
        modifiers: sent(MODIFIERS, sends_modifier),
        aliases: Aliases {
            keys: aliases_in(KEY_ALIASES, sends_key),
            media_keys: aliases_in(MEDIA_KEY_ALIASES, sends_media_key),
            modifiers: modifier_aliases(),
        },
        labels: LABELS.iter().copied().collect(),
    }
}

/// Every other spelling of a modifier this system holds: with a side, by a
/// shorter name, or both, as [`modifier_flag`] reads them.
fn modifier_aliases() -> BTreeMap<String, &'static str> {
    let spellings = MODIFIERS
        .iter()
        .map(|entry| (entry.0, entry.0))
        .chain(MODIFIER_ALIASES.iter().copied());
    let mut aliases = BTreeMap::new();
    for (spelling, listed) in spellings {
        for side in [""].into_iter().chain(SIDES) {
            let name = format!("{side}{spelling}");
            if name != listed && sends_modifier(listed) {
                aliases.insert(name, listed);
            }
        }
    }
    aliases
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Karabiner names a side and macOS does not, so both must work: a person
    /// copying from Karabiner should not have to know the difference.
    #[test]
    fn a_modifier_may_name_a_side() {
        for name in ["command", "left_command", "right_command", "cmd"] {
            assert_eq!(modifier_flag(name), Some(FLAG_COMMAND), "{name}");
        }
        assert_eq!(modifier_flag("left_control"), Some(FLAG_CONTROL));
        assert_eq!(modifier_flag("right_option"), Some(FLAG_OPTION));
        assert_eq!(modifier_flag("alt"), Some(FLAG_OPTION));
        assert_eq!(modifier_flag("left_nonsense"), None);
    }

    #[test]
    fn maps_letters_and_digits() {
        assert_eq!(key_code("a"), Some(0));
        assert_eq!(key_code("z"), Some(6));
        assert_eq!(key_code("0"), Some(29));
        assert_eq!(key_code("4"), Some(21));
    }

    #[test]
    fn maps_named_keys() {
        assert_eq!(key_code("return_or_enter"), Some(36));
        assert_eq!(key_code("tab"), Some(48));
        assert_eq!(key_code("spacebar"), Some(49));
        assert_eq!(key_code("escape"), Some(53));
        assert_eq!(key_code("delete_or_backspace"), Some(51));
        assert_eq!(key_code("left_arrow"), Some(123));
        assert_eq!(key_code("up_arrow"), Some(126));
        assert_eq!(key_code("page_up"), Some(116));
        assert_eq!(key_code("page_down"), Some(121));
        assert_eq!(key_code("home"), Some(115));
        assert_eq!(key_code("end"), Some(119));
        assert_eq!(key_code("delete_forward"), Some(117));
        assert_eq!(key_code("f1"), Some(122));
        assert_eq!(key_code("f12"), Some(111));
        // Past f12 the codes stop being sequential, so each is worth pinning.
        assert_eq!(key_code("f13"), Some(105));
        assert_eq!(key_code("f17"), Some(64));
        assert_eq!(key_code("f18"), Some(79));
        assert_eq!(key_code("f20"), Some(90));
        assert_eq!(key_code("f21"), None);
    }

    /// Every key_code name Karabiner-Elements offers, taken from its own
    /// simple_modifications.json. A name read off Karabiner-EventViewer must
    /// either work here or say why it cannot. Neither answer may be silence.
    const KARABINER_KEY_CODES: &[&str] = &[
        "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "again",
        "alternate_erase", "apple_display_brightness_decrement",
        "apple_display_brightness_increment",
        "apple_top_case_display_brightness_decrement",
        "apple_top_case_display_brightness_increment", "application", "b",
        "backslash", "c", "cancel", "caps_lock", "clear", "clear_or_again",
        "close_bracket", "comma", "copy", "cr_sel_or_props", "cut", "d",
        "dashboard", "delete_forward", "delete_or_backspace",
        "display_brightness_decrement", "display_brightness_increment",
        "down_arrow", "e", "end", "equal_sign", "escape", "ex_sel",
        "execute", "f", "f1", "f10", "f11", "f12", "f13", "f14", "f15",
        "f16", "f17", "f18", "f19", "f2", "f20", "f21", "f22", "f23", "f24",
        "f3", "f4", "f5", "f6", "f7", "f8", "f9", "fastforward", "find",
        "fn", "g", "grave_accent_and_tilde", "h", "help", "home", "hyphen",
        "i", "illumination_decrement", "illumination_increment", "insert",
        "international1", "international2", "international3",
        "international4", "international5", "international6",
        "international7", "international8", "international9", "j",
        "japanese_eisuu", "japanese_kana", "japanese_pc_katakana",
        "japanese_pc_nfer", "japanese_pc_xfer", "k", "keypad_0", "keypad_1",
        "keypad_2", "keypad_3", "keypad_4", "keypad_5", "keypad_6",
        "keypad_7", "keypad_8", "keypad_9", "keypad_asterisk",
        "keypad_comma", "keypad_enter", "keypad_equal_sign",
        "keypad_equal_sign_as400", "keypad_hyphen", "keypad_num_lock",
        "keypad_period", "keypad_plus", "keypad_slash", "l", "lang1",
        "lang2", "lang3", "lang4", "lang5", "lang6", "lang7", "lang8",
        "lang9", "launchpad", "left_alt", "left_arrow", "left_command",
        "left_control", "left_gui", "left_option", "left_shift",
        "locking_caps_lock", "locking_num_lock", "locking_scroll_lock", "m",
        "menu", "mission_control", "mute", "n", "non_us_backslash",
        "non_us_pound", "o", "open_bracket", "oper", "out", "p",
        "page_down", "page_up", "paste", "pause", "period", "play_or_pause",
        "power", "print_screen", "prior", "q", "quote", "r", "return",
        "return_or_enter", "rewind", "right_alt", "right_arrow",
        "right_command", "right_control", "right_gui", "right_option",
        "right_shift", "s", "scroll_lock", "select", "semicolon",
        "separator", "slash", "spacebar", "stop", "sys_req_or_attention",
        "t", "tab", "u", "undo", "up_arrow", "v",
        "vk_consumer_brightness_down", "vk_consumer_brightness_up",
        "vk_consumer_illumination_down", "vk_consumer_illumination_up",
        "vk_consumer_next", "vk_consumer_play", "vk_consumer_previous",
        "vk_dashboard", "vk_launchpad", "vk_mission_control", "vk_none",
        "volume_decrement", "volume_down", "volume_increment", "volume_up",
        "w", "x", "y", "z",
    ];

    #[test]
    fn every_karabiner_name_is_answered() {
        for name in KARABINER_KEY_CODES {
            let known = sends_key(name);
            let explained = unsupported_key(name).is_some();
            assert!(
                known || explained,
                "Karabiner offers {name}, and this program neither sends it nor says why"
            );
            assert!(
                !(known && explained),
                "{name} is both sent and refused"
            );
        }
    }

    #[test]
    fn a_refusal_names_the_media_key_to_use_instead() {
        let why = unsupported_key("volume_up").expect("volume_up is a Karabiner name");
        assert!(why.contains("consumer_key_code"), "point at the fix: {why}");
        assert!(why.contains("volume_increment"), "name the key: {why}");
    }

    #[test]
    fn a_modifier_is_also_a_key_in_its_own_right() {
        // Karabiner treats a modifier as a key, and so must this. The sides
        // differ here, unlike in the modifiers list, because macOS has a
        // keycode for each.
        assert_eq!(key_code("left_command"), Some(55));
        assert_eq!(key_code("right_command"), Some(54));
        assert_eq!(key_code("caps_lock"), Some(57));
        assert_eq!(key_code("fn"), Some(63));
        assert_eq!(key_code("left_gui"), key_code("left_command"));
        assert_eq!(key_code("left_alt"), key_code("left_option"));
    }

    #[test]
    fn maps_the_keypad() {
        assert_eq!(key_code("keypad_0"), Some(82));
        assert_eq!(key_code("keypad_7"), Some(89));
        assert_eq!(key_code("keypad_8"), Some(91), "8 and 9 skip a number");
        assert_eq!(key_code("keypad_9"), Some(92));
        assert_eq!(key_code("keypad_enter"), Some(76));
        // A Mac keypad has clear where a PC keypad has num lock.
        assert_eq!(key_code("keypad_num_lock"), key_code("clear"));
    }

    #[test]
    fn rejects_an_unknown_key_name() {
        assert_eq!(key_code("meta"), None);
        assert_eq!(key_code(""), None);
        assert_eq!(key_code("A"), None, "names are lowercase");
    }

    #[test]
    fn maps_media_keys() {
        assert_eq!(consumer_code("play_or_pause"), Some(16));
        assert_eq!(consumer_code("volume_increment"), Some(0));
        assert_eq!(consumer_code("volume_decrement"), Some(1));
        assert_eq!(consumer_code("mute"), Some(7));
        assert_eq!(consumer_code("scan_next_track"), Some(17));
        assert_eq!(consumer_code("scan_previous_track"), Some(18));
        assert_eq!(consumer_code("fast_forward"), Some(19));
        assert_eq!(consumer_code("rewind"), Some(20));
        assert_eq!(consumer_code("play"), None, "not a Karabiner name");
    }

    #[test]
    fn maps_modifiers() {
        assert_eq!(modifier_flag("command"), Some(FLAG_COMMAND));
        assert_eq!(modifier_flag("shift"), Some(FLAG_SHIFT));
        assert_eq!(modifier_flag("option"), Some(FLAG_OPTION));
        assert_eq!(modifier_flag("control"), Some(FLAG_CONTROL));
        assert_eq!(modifier_flag("fn"), Some(FLAG_FN));
        assert_eq!(modifier_flag("meta"), None);
    }

    #[test]
    fn every_letter_digit_and_f_key_maps_to_its_exact_code() {
        // Sampling would miss a swap: 6 and 5 are deliberately out of the
        // usual order (22, 23), and the F-keys are not sequential. A
        // regression could "fix" either into something that looks right but
        // sends the wrong key.
        let letters = [
            ("a", 0), ("s", 1), ("d", 2), ("f", 3), ("h", 4), ("g", 5),
            ("z", 6), ("x", 7), ("c", 8), ("v", 9), ("b", 11), ("q", 12),
            ("w", 13), ("e", 14), ("r", 15), ("y", 16), ("t", 17),
            ("o", 31), ("u", 32), ("i", 34), ("p", 35), ("l", 37), ("j", 38),
            ("k", 40), ("n", 45), ("m", 46),
        ];
        for (name, code) in letters {
            assert_eq!(key_code(name), Some(code), "letter {name}");
        }

        let digits = [
            ("1", 18), ("2", 19), ("3", 20), ("4", 21), ("6", 22), ("5", 23),
            ("9", 25), ("7", 26), ("8", 28), ("0", 29),
        ];
        for (name, code) in digits {
            assert_eq!(key_code(name), Some(code), "digit {name}");
        }

        let f_keys = [
            ("f1", 122), ("f2", 120), ("f3", 99), ("f4", 118), ("f5", 96),
            ("f6", 97), ("f7", 98), ("f8", 100), ("f9", 101), ("f10", 109),
            ("f11", 103), ("f12", 111),
            ("f13", 105), ("f14", 107), ("f15", 113), ("f16", 106),
            ("f17", 64), ("f18", 79), ("f19", 80), ("f20", 90),
        ];
        for (name, code) in f_keys {
            assert_eq!(key_code(name), Some(code), "f-key {name}");
        }
    }

    #[test]
    fn every_advertised_name_resolves() {
        // The spec lists these; a typo in the table would be invisible otherwise.
        for name in [
            "a", "m", "z", "0", "9", "f1", "f12", "return_or_enter", "tab",
            "spacebar", "escape", "delete_or_backspace", "left_arrow",
            "right_arrow", "up_arrow", "down_arrow", "hyphen", "equal_sign",
            "comma", "period", "slash", "semicolon", "quote", "open_bracket",
            "close_bracket", "backslash", "grave_accent_and_tilde",
        ] {
            assert!(key_code(name).is_some(), "missing key name: {name}");
        }
        for name in [
            "play_or_pause", "volume_increment", "volume_decrement", "mute",
            "fast_forward", "rewind", "scan_next_track", "scan_previous_track",
        ] {
            assert!(consumer_code(name).is_some(), "missing media name: {name}");
        }
    }

    // --- the window's list of keys (D3) -----------------------------------

    /// A name the window lists is one the file takes, and it is listed once:
    /// no name twice, and no key twice under two names.
    #[test]
    fn every_listed_name_is_one_the_file_takes_once() {
        let names = names();
        for key in &names.keys {
            assert!(sends_key(key), "the window lists {key}");
        }
        for media in &names.media_keys {
            assert!(consumer_code(media).is_some(), "the window lists {media}");
        }
        for modifier in &names.modifiers {
            assert!(
                modifier_flag(modifier).is_some(),
                "the window lists {modifier}"
            );
        }

        let spellings: std::collections::BTreeSet<&str> = names.keys.iter().copied().collect();
        assert_eq!(spellings.len(), names.keys.len(), "a name is listed twice");
        // Every key on the Mac, and on Windows all but the PC's own keys,
        // which have no macOS keycode.
        let with_codes: Vec<u16> = names.keys.iter().filter_map(|key| key_code(key)).collect();
        let codes: std::collections::BTreeSet<u16> = with_codes.iter().copied().collect();
        assert_eq!(codes.len(), with_codes.len(), "a key is listed twice");
        assert_eq!(
            with_codes.len() + keys_only_here().count(),
            names.keys.len(),
            "a key the table lists has no code"
        );
        let media: std::collections::BTreeSet<u8> = names
            .media_keys
            .iter()
            .filter_map(|key| consumer_code(key))
            .collect();
        assert_eq!(
            media.len(),
            names.media_keys.len(),
            "a media key is listed twice"
        );
        let flags: std::collections::BTreeSet<u64> = names
            .modifiers
            .iter()
            .filter_map(|m| modifier_flag(m))
            .collect();
        assert_eq!(
            flags.len(),
            names.modifiers.len(),
            "a modifier is listed twice"
        );
    }

    /// Every other spelling the file takes is the same key as the name it
    /// stands for, and is not listed itself, so a keystroke read from the
    /// file can be shown against the list.
    #[test]
    fn every_other_spelling_is_the_key_it_stands_for() {
        let names = names();
        for (alias, listed) in &names.aliases.keys {
            assert!(names.keys.contains(listed), "{alias} stands for {listed}");
            assert!(!names.keys.contains(&alias.as_str()), "{alias} is listed");
            assert_eq!(key_code(alias), key_code(listed), "{alias}");
        }
        for (alias, listed) in &names.aliases.media_keys {
            assert!(
                names.media_keys.contains(listed),
                "{alias} stands for {listed}"
            );
            assert_eq!(consumer_code(alias), consumer_code(listed), "{alias}");
        }
        for (alias, listed) in &names.aliases.modifiers {
            assert!(
                names.modifiers.contains(listed),
                "{alias} stands for {listed}"
            );
            assert!(
                !names.modifiers.contains(&alias.as_str()),
                "{alias} is listed"
            );
            assert_eq!(modifier_flag(alias), modifier_flag(listed), "{alias}");
        }
    }

    /// Nothing the file takes is missing from the list: every Karabiner key
    /// name this program sends, and every spelling of a modifier it reads.
    #[test]
    fn the_list_leaves_out_nothing_the_file_takes() {
        let names = names();
        for name in KARABINER_KEY_CODES {
            if sends_key(name) {
                assert!(
                    names.keys.contains(name) || names.aliases.keys.contains_key(*name),
                    "{name} is sent, and the window does not list it"
                );
            }
            if sends_media_key(name) {
                assert!(
                    names.media_keys.contains(name) || names.aliases.media_keys.contains_key(*name),
                    "{name} is a media key the window does not list"
                );
            }
        }
        for side in ["", "left_", "right_"] {
            for word in [
                "command", "cmd", "shift", "option", "alt", "opt", "control", "ctrl", "fn", "gui",
                "meta", "hyper", "super",
            ] {
                let name = format!("{side}{word}");
                let listed = names.modifiers.contains(&name.as_str())
                    || names.aliases.modifiers.contains_key(&name);
                assert_eq!(sends_modifier(&name), listed, "{name}");
            }
        }
    }

    /// Record keys waits past a modifier for the key it goes with.
    #[test]
    fn the_modifier_keys_are_the_ones_the_list_holds_as_keys() {
        let names = names();
        let modifier_keys: Vec<&str> = names
            .keys
            .iter()
            .copied()
            .filter(|key| is_modifier_key(key))
            .collect();
        let every_one = [
            "left_control",
            "left_option",
            "left_shift",
            "left_command",
            "right_control",
            "right_option",
            "right_shift",
            "right_command",
            "caps_lock",
            "fn",
        ];
        // All ten on the Mac. Windows has no fn to send.
        let sent: Vec<&str> = every_one.into_iter().filter(|key| sends_key(key)).collect();
        assert_eq!(modifier_keys, sent);
        assert!(!is_modifier_key("k"));
        assert!(
            !is_modifier_key("command"),
            "a modifier's own name is no key"
        );
    }
}
