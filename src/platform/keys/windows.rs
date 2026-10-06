//! Posting keys on Windows, with SendInput, and the names Windows cannot send.
//!
//! The names are Karabiner's, as on the Mac (W2). Each key the tables of
//! [`crate::keys`] list stands for a Windows virtual key here, or is refused
//! with why: no keyboard reports fn to Windows, for one. Command is the Win
//! key and option is Alt, and the window labels the modifiers Ctrl, Alt,
//! Shift and Win.
//!
//! A virtual key follows the keyboard layout: VK_A is the key that types a.
//! A macOS keycode is a place on the keyboard instead. With a US layout the
//! two agree.

use crate::config::Keystroke;
use crate::keys::{self, FLAG_COMMAND, FLAG_CONTROL, FLAG_OPTION, FLAG_SHIFT};
use windows::Win32::UI::Input::KeyboardAndMouse::*;

/// A key as SendInput takes it: its virtual key, and whether a real key
/// sends it with the extended bit, as the arrows do. Keypad enter is return
/// with the extended bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualKey {
    pub code: u16,
    pub extended: bool,
    /// The scan code a real key sends, for a key `MapVirtualKeyW` gives
    /// another: see [`scanned`]. Zero for every other key, whose scan code
    /// `MapVirtualKeyW` gives.
    pub scan: u16,
}

const fn vk(code: VIRTUAL_KEY) -> VirtualKey {
    VirtualKey {
        code: code.0,
        extended: false,
        scan: 0,
    }
}

/// A key a real keyboard sends with the extended bit. Windows names them:
/// the arrows and the keys above them, keypad enter and slash, num lock, the
/// right Ctrl and Alt, and the Win keys. The bit is what tells an arrow from
/// the same key on the keypad, and keypad enter from return.
const fn extended(code: VIRTUAL_KEY) -> VirtualKey {
    VirtualKey {
        code: code.0,
        extended: true,
        scan: 0,
    }
}

/// A key with the scan code a real key sends, where `MapVirtualKeyW` gives
/// another. It gives Print Screen 0x54, which is Alt and Print Screen, where
/// the key alone sends 0x37, and Pause nothing, where the key sends 0x45.
const fn scanned(key: VirtualKey, scan: u16) -> VirtualKey {
    VirtualKey { scan, ..key }
}

/// The virtual key each key stands for on Windows, by the name the table of
/// [`crate::keys`] lists it under, in that table's order. Every key it lists
/// is here, or in [`REFUSED_KEYS`].
#[rustfmt::skip]
const VIRTUAL_KEYS: &[(&str, VirtualKey)] = &[
    ("a", vk(VK_A)), ("b", vk(VK_B)), ("c", vk(VK_C)), ("d", vk(VK_D)), ("e", vk(VK_E)),
    ("f", vk(VK_F)), ("g", vk(VK_G)), ("h", vk(VK_H)), ("i", vk(VK_I)), ("j", vk(VK_J)),
    ("k", vk(VK_K)), ("l", vk(VK_L)), ("m", vk(VK_M)), ("n", vk(VK_N)), ("o", vk(VK_O)),
    ("p", vk(VK_P)), ("q", vk(VK_Q)), ("r", vk(VK_R)), ("s", vk(VK_S)), ("t", vk(VK_T)),
    ("u", vk(VK_U)), ("v", vk(VK_V)), ("w", vk(VK_W)), ("x", vk(VK_X)), ("y", vk(VK_Y)),
    ("z", vk(VK_Z)),
    ("0", vk(VK_0)), ("1", vk(VK_1)), ("2", vk(VK_2)), ("3", vk(VK_3)), ("4", vk(VK_4)),
    ("5", vk(VK_5)), ("6", vk(VK_6)), ("7", vk(VK_7)), ("8", vk(VK_8)), ("9", vk(VK_9)),
    ("return_or_enter", vk(VK_RETURN)), ("tab", vk(VK_TAB)), ("spacebar", vk(VK_SPACE)),
    ("delete_or_backspace", vk(VK_BACK)), ("escape", vk(VK_ESCAPE)),
    // The keys around the letters. Their virtual keys follow the layout, and
    // these are where a US layout puts them.
    ("hyphen", vk(VK_OEM_MINUS)), ("equal_sign", vk(VK_OEM_PLUS)),
    ("open_bracket", vk(VK_OEM_4)), ("close_bracket", vk(VK_OEM_6)),
    ("backslash", vk(VK_OEM_5)), ("semicolon", vk(VK_OEM_1)), ("quote", vk(VK_OEM_7)),
    ("grave_accent_and_tilde", vk(VK_OEM_3)), ("comma", vk(VK_OEM_COMMA)),
    ("period", vk(VK_OEM_PERIOD)), ("slash", vk(VK_OEM_2)),
    // The key an ISO keyboard has beside the left shift.
    ("non_us_backslash", vk(VK_OEM_102)),
    ("up_arrow", extended(VK_UP)), ("down_arrow", extended(VK_DOWN)),
    ("left_arrow", extended(VK_LEFT)), ("right_arrow", extended(VK_RIGHT)),
    ("home", extended(VK_HOME)), ("end", extended(VK_END)), ("page_up", extended(VK_PRIOR)),
    ("page_down", extended(VK_NEXT)), ("delete_forward", extended(VK_DELETE)),
    ("f1", vk(VK_F1)), ("f2", vk(VK_F2)), ("f3", vk(VK_F3)), ("f4", vk(VK_F4)),
    ("f5", vk(VK_F5)), ("f6", vk(VK_F6)), ("f7", vk(VK_F7)), ("f8", vk(VK_F8)),
    ("f9", vk(VK_F9)), ("f10", vk(VK_F10)), ("f11", vk(VK_F11)), ("f12", vk(VK_F12)),
    // As on the Mac, nothing else on the PC sends these, which makes them
    // good for a band button.
    ("f13", vk(VK_F13)), ("f14", vk(VK_F14)), ("f15", vk(VK_F15)), ("f16", vk(VK_F16)),
    ("f17", vk(VK_F17)), ("f18", vk(VK_F18)), ("f19", vk(VK_F19)), ("f20", vk(VK_F20)),
    ("keypad_0", vk(VK_NUMPAD0)), ("keypad_1", vk(VK_NUMPAD1)), ("keypad_2", vk(VK_NUMPAD2)),
    ("keypad_3", vk(VK_NUMPAD3)), ("keypad_4", vk(VK_NUMPAD4)), ("keypad_5", vk(VK_NUMPAD5)),
    ("keypad_6", vk(VK_NUMPAD6)), ("keypad_7", vk(VK_NUMPAD7)), ("keypad_8", vk(VK_NUMPAD8)),
    ("keypad_9", vk(VK_NUMPAD9)),
    ("keypad_period", vk(VK_DECIMAL)), ("keypad_enter", extended(VK_RETURN)),
    ("keypad_plus", vk(VK_ADD)), ("keypad_hyphen", vk(VK_SUBTRACT)),
    ("keypad_asterisk", vk(VK_MULTIPLY)), ("keypad_slash", extended(VK_DIVIDE)),
    // A PC keypad has num lock where a Mac keypad has clear.
    ("keypad_num_lock", extended(VK_NUMLOCK)),
    ("left_control", vk(VK_LCONTROL)), ("left_option", vk(VK_LMENU)),
    ("left_shift", vk(VK_LSHIFT)), ("left_command", extended(VK_LWIN)),
    ("right_control", extended(VK_RCONTROL)), ("right_option", extended(VK_RMENU)),
    ("right_shift", vk(VK_RSHIFT)), ("right_command", extended(VK_RWIN)),
    ("caps_lock", vk(VK_CAPITAL)),
];

/// Keys a PC keyboard has and a Mac keyboard lacks, by the names Karabiner
/// gives them. The table of [`crate::keys`] does not list them, since macOS
/// has no keycode for them, and Windows sends each (W2). The window lists
/// them after the others.
#[rustfmt::skip]
const PC_KEYS: &[(&str, VirtualKey)] = &[
    ("insert", extended(VK_INSERT)),
    ("print_screen", scanned(extended(VK_SNAPSHOT), 0x37)),
    ("scroll_lock", vk(VK_SCROLL)), ("pause", scanned(vk(VK_PAUSE), 0x45)),
    // The key that opens a context menu, beside the right Ctrl.
    ("application", extended(VK_APPS)),
    ("f21", vk(VK_F21)), ("f22", vk(VK_F22)), ("f23", vk(VK_F23)), ("f24", vk(VK_F24)),
];

/// The keys Windows sends that the table of [`crate::keys`] does not list:
/// [`PC_KEYS`].
pub fn keys_only_here() -> impl Iterator<Item = &'static str> {
    PC_KEYS.iter().map(|entry| entry.0)
}

/// Every key Windows sends, and the virtual key it stands for.
fn every_key() -> impl Iterator<Item = &'static (&'static str, VirtualKey)> {
    VIRTUAL_KEYS.iter().chain(PC_KEYS)
}

/// Why Windows cannot send fn, as a key or as a modifier.
const NO_FN: &str = "Windows has no key event for fn.";

/// Why BandBridge does not send the keys of Japanese keyboards on Windows.
const JAPANESE: &str = "Mi Band Bridge does not send the keys of Japanese keyboards on Windows.";

/// The keys the table of [`crate::keys`] lists that Windows cannot send,
/// each with why.
const REFUSED_KEYS: &[(&str, &str)] = &[
    ("help", "A PC keyboard has no help key."),
    ("keypad_equal_sign", "A PC keypad has no equal sign key."),
    ("keypad_comma", "A PC keypad has no comma key."),
    ("fn", NO_FN),
    ("japanese_eisuu", JAPANESE),
    ("japanese_kana", JAPANESE),
    ("international1", JAPANESE),
    ("international3", JAPANESE),
];

/// The media keys a PC keyboard has, and the virtual key each stands for.
/// A real media key sends the extended bit, so these go out with it.
const MEDIA_VIRTUAL_KEYS: &[(&str, u16)] = &[
    ("volume_increment", VK_VOLUME_UP.0),
    ("volume_decrement", VK_VOLUME_DOWN.0),
    ("mute", VK_VOLUME_MUTE.0),
    ("play_or_pause", VK_MEDIA_PLAY_PAUSE.0),
    ("scan_next_track", VK_MEDIA_NEXT_TRACK.0),
    ("scan_previous_track", VK_MEDIA_PREV_TRACK.0),
];

/// Why Windows cannot send a brightness key.
const NO_BRIGHTNESS: &str = "Windows has no key for display brightness.";

/// Why Windows cannot send a keyboard lighting key.
const NO_LIGHTING: &str = "Windows has no key for keyboard lighting.";

/// The media keys the table of [`crate::keys`] lists that Windows cannot
/// send, each with why.
const REFUSED_MEDIA_KEYS: &[(&str, &str)] = &[
    ("fast_forward", "Windows has no fast forward key."),
    ("rewind", "Windows has no rewind key."),
    ("eject", "Windows has no eject key."),
    ("display_brightness_increment", NO_BRIGHTNESS),
    ("display_brightness_decrement", NO_BRIGHTNESS),
    ("illumination_increment", NO_LIGHTING),
    ("illumination_decrement", NO_LIGHTING),
];

/// The key each modifier holds down, by its name in [`VIRTUAL_KEYS`], in the
/// order a keystroke presses them, which is the Mac's: control, option,
/// shift, command. Command is the Win key and option is Alt (W2). fn has no
/// key, and holds nothing.
const MODIFIER_KEYS: &[(u64, &str)] = &[
    (FLAG_CONTROL, "left_control"),
    (FLAG_OPTION, "left_option"),
    (FLAG_SHIFT, "left_shift"),
    (FLAG_COMMAND, "left_command"),
];

/// The word the window shows for each modifier on Windows (W2).
pub const LABELS: &[(&str, &str)] = &[
    ("command", "Win"),
    ("shift", "Shift"),
    ("option", "Alt"),
    ("control", "Ctrl"),
];

/// What to do about a key that neither the table of [`crate::keys`] nor
/// [`PC_KEYS`] lists. The window's list and Record keys offer only keys
/// BandBridge sends.
const NOT_SENT: &str = "Choose a key from the list in the window.";

/// Whether this process may post input events. Windows asks no permission
/// to send keys, so it may. A key sent to an app that runs as administrator
/// is dropped without a word, which no call here can tell.
pub fn trusted(_prompt: bool) -> bool {
    true
}

/// The name a virtual key stands for, for Record keys: [`virtual_key`] the
/// other way. Return and keypad enter share a virtual key, and the extended
/// bit tells them apart. Any other key reads the same either way: the up
/// arrow is the up arrow, from the arrows or from the keypad.
pub fn key_name(code: u16, extended: bool) -> Option<&'static str> {
    let mut same = every_key().filter(|(_, key)| key.code == code);
    let first = same.next()?;
    let exact = std::iter::once(first)
        .chain(same)
        .find(|(_, key)| key.extended == extended);
    Some(exact.unwrap_or(first).0)
}

/// The name a media virtual key stands for: [`media_virtual_key`] the other
/// way.
pub fn media_key_name(code: u16) -> Option<&'static str> {
    MEDIA_VIRTUAL_KEYS
        .iter()
        .find(|entry| entry.1 == code)
        .map(|entry| entry.0)
}

/// The keystroke a key a person pressed stands for, as the window's Record
/// keys names it: the key, and the modifiers `flags` holds, in the order the
/// list gives them. The flags are the bits of [`crate::keys`], which the
/// recorder sets for the modifiers it saw held. fn is never one. None for a
/// key this program has no name for, and so cannot send.
pub fn heard_key(code: u16, extended: bool, flags: u64) -> Option<Keystroke> {
    let key = key_name(code, extended)?;
    let modifiers = keys::MODIFIERS
        .iter()
        .filter(|(name, flag)| flags & flag != 0 && keys::sends_modifier(name))
        .map(|(name, _)| name.to_string())
        .collect();
    Some(Keystroke {
        key_code: Some(key.to_string()),
        consumer_key_code: None,
        modifiers,
    })
}

/// The keystroke a media key a person pressed stands for, with no
/// modifiers, which a media key never takes.
pub fn heard_media_key(code: u16) -> Option<Keystroke> {
    Some(Keystroke {
        key_code: None,
        consumer_key_code: Some(media_key_name(code)?.to_string()),
        modifiers: Vec::new(),
    })
}

/// Why Windows cannot send a key the file may name, or None when it can.
///
/// A key the table of [`crate::keys`] lists is refused only when Windows
/// cannot send it, and a key of [`PC_KEYS`] is sent. Any other name
/// Karabiner gives a key is refused, in words true on Windows. None for a
/// name nobody gives a key.
#[rustfmt::skip]
pub fn unsupported_key(name: &str) -> Option<&'static str> {
    if let Some(listed) = keys::listed_key(name) {
        return why(REFUSED_KEYS, listed);
    }
    let reason = match name {
        "volume_up" | "volume_increment" => {
            "This is a media key. Set consumer_key_code = \"volume_increment\"."
        }
        "volume_down" | "volume_decrement" => {
            "This is a media key. Set consumer_key_code = \"volume_decrement\"."
        }
        "mute" => "This is a media key. Set consumer_key_code = \"mute\".",
        "play_or_pause" | "vk_consumer_play" => {
            "This is a media key. Set consumer_key_code = \"play_or_pause\"."
        }
        "vk_consumer_next" => {
            "This is a media key. Set consumer_key_code = \"scan_next_track\"."
        }
        "vk_consumer_previous" => {
            "This is a media key. Set consumer_key_code = \"scan_previous_track\"."
        }
        "rewind" => "Windows has no rewind key.",
        "fastforward" => "Windows has no fast forward key.",
        "display_brightness_increment"
        | "display_brightness_decrement"
        | "apple_display_brightness_increment"
        | "apple_display_brightness_decrement"
        | "apple_top_case_display_brightness_increment"
        | "apple_top_case_display_brightness_decrement"
        | "vk_consumer_brightness_up"
        | "vk_consumer_brightness_down" => NO_BRIGHTNESS,
        "illumination_increment"
        | "illumination_decrement"
        | "vk_consumer_illumination_up"
        | "vk_consumer_illumination_down" => NO_LIGHTING,

        "mission_control" | "vk_mission_control" | "launchpad" | "vk_launchpad"
        | "dashboard" | "vk_dashboard" => {
            "This opens a macOS feature, and Windows has no key for it."
        }

        "again" | "alternate_erase" | "cancel" | "clear_or_again" | "copy" | "cr_sel_or_props"
        | "cut" | "execute" | "ex_sel" | "find" | "keypad_equal_sign_as400"
        | "locking_caps_lock" | "locking_num_lock" | "locking_scroll_lock" | "menu"
        | "non_us_pound" | "oper" | "out" | "paste" | "power" | "select" | "separator"
        | "stop" | "sys_req_or_attention" | "undo" | "international2" | "international4"
        | "international5" | "international6" | "international7" | "international8"
        | "international9" | "lang3" | "lang4" | "lang5" | "lang6" | "lang7" | "lang8"
        | "lang9" | "japanese_pc_katakana" | "japanese_pc_nfer" | "japanese_pc_xfer" => {
            NOT_SENT
        }

        "vk_none" => "this disables a key in Karabiner. It sends nothing.",

        _ => return None,
    };
    Some(reason)
}

/// Why Windows cannot send a media key the table of [`crate::keys`] lists,
/// or None when it can, or does not list it.
pub fn unsupported_media_key(name: &str) -> Option<&'static str> {
    why(REFUSED_MEDIA_KEYS, keys::listed_media_key(name)?)
}

/// Why Windows cannot hold a modifier, in any spelling: fn alone.
pub fn unsupported_modifier(name: &str) -> Option<&'static str> {
    (keys::listed_modifier(name)? == "fn").then_some(NO_FN)
}

/// The reason a table of refusals gives for a name.
fn why(refused: &[(&str, &'static str)], name: &str) -> Option<&'static str> {
    refused
        .iter()
        .find(|entry| entry.0 == name)
        .map(|entry| entry.1)
}

/// Fire a checked keystroke. The loader checks every name, so an unknown one
/// here means the keystroke never went through it, and nothing is sent.
pub fn fire(keystroke: &Keystroke) -> Result<(), String> {
    if let Some(name) = &keystroke.consumer_key_code {
        let code =
            media_virtual_key(name).ok_or_else(|| format!("unknown consumer_key_code {name}"))?;
        return tap_media(code);
    }
    let Some(name) = &keystroke.key_code else {
        return Err("a keystroke needs key_code or consumer_key_code".to_string());
    };
    let key = virtual_key(name).ok_or_else(|| format!("unknown key_code {name}"))?;
    let flags = keystroke
        .modifiers
        .iter()
        .filter_map(|m| keys::modifier_flag(m))
        .fold(0u64, |acc, f| acc | f);
    tap_key(key, flags)
}

/// Send one keystroke, holding its modifiers down around it, as one batch.
pub fn tap_key(key: VirtualKey, flags: u64) -> Result<(), String> {
    send(&strokes(key, flags))
}

/// Send one media key, down and up, as one batch.
pub fn tap_media(code: u16) -> Result<(), String> {
    let key = VirtualKey {
        code,
        extended: true,
        scan: 0,
    };
    send(&[Stroke::down(key), Stroke::up(key)])
}

/// The virtual key a key name stands for, in any spelling the file takes.
fn virtual_key(name: &str) -> Option<VirtualKey> {
    let name = keys::listed_key(name).unwrap_or(name);
    every_key()
        .find(|entry| entry.0 == name)
        .map(|entry| entry.1)
}

/// The virtual key a media key name stands for, in any spelling the file
/// takes.
fn media_virtual_key(name: &str) -> Option<u16> {
    let listed = keys::listed_media_key(name)?;
    MEDIA_VIRTUAL_KEYS
        .iter()
        .find(|entry| entry.0 == listed)
        .map(|entry| entry.1)
}

/// One key going down or up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stroke {
    key: VirtualKey,
    up: bool,
}

impl Stroke {
    fn down(key: VirtualKey) -> Self {
        Self { key, up: false }
    }

    fn up(key: VirtualKey) -> Self {
        Self { key, up: true }
    }
}

/// A keystroke as key events: the modifiers go down, in the order
/// [`MODIFIER_KEYS`] gives, then the key goes down and up, then the
/// modifiers come up in the reverse order. Setting a flag on the key is not
/// the same as holding the modifier, which some shortcuts watch for.
fn strokes(key: VirtualKey, flags: u64) -> Vec<Stroke> {
    let held: Vec<VirtualKey> = MODIFIER_KEYS
        .iter()
        .filter(|(flag, _)| flags & flag != 0)
        .filter_map(|(_, name)| virtual_key(name))
        .collect();
    let mut all: Vec<Stroke> = held.iter().copied().map(Stroke::down).collect();
    all.push(Stroke::down(key));
    all.push(Stroke::up(key));
    all.extend(held.iter().rev().copied().map(Stroke::up));
    all
}

/// The key events that let go of every key these events pressed and did not
/// release, the last pressed first.
fn releases(sent: &[Stroke]) -> Vec<Stroke> {
    let mut held: Vec<VirtualKey> = Vec::new();
    for stroke in sent {
        if stroke.up {
            held.retain(|key| *key != stroke.key);
        } else {
            held.push(stroke.key);
        }
    }
    held.into_iter().rev().map(Stroke::up).collect()
}

/// A virtual key no key has. Tapped while Win or Alt is down, it makes their
/// release part of a shortcut, so Windows opens neither Start nor a menu
/// bar when they come up alone.
const BLANK: VirtualKey = VirtualKey {
    code: 0xE8,
    extended: false,
    scan: 0,
};

/// The extra information on every key event BandBridge sends, so the window's
/// Record keys can tell them from the keys a person presses, as the Mac's tap
/// tells them by the process that posted them. No other program is known to
/// use this value, and Record keys checks the injected bit beside it.
pub const SENT_BY_BANDBRIDGE: usize = 0x4242_4242;

/// Tap [`BLANK`] while a modifier is down, so the modifier's release is part
/// of a shortcut: Windows then opens neither Start nor a menu bar, and keeps
/// the input language. The window's Record keys taps it when it swallows the
/// key it records, since the modifiers held with that key still reach the
/// system. The tap carries [`SENT_BY_BANDBRIDGE`], so Record keys leaves it
/// alone.
pub fn tap_blank() -> Result<(), String> {
    send(&[Stroke::down(BLANK), Stroke::up(BLANK)])
}

/// Send key events as one batch (Review Focus 2).
///
/// One SendInput call puts the whole batch into the input stream, with no
/// key a person types between its events. When Windows takes only the
/// first part of it, a modifier that went down in that part would stay held
/// system-wide, so every key that part pressed is let go of at once. When
/// Windows does not take all of that either, the error names each key that
/// may still be down.
fn send(strokes: &[Stroke]) -> Result<(), String> {
    let taken = send_now(strokes);
    if taken == strokes.len() {
        return Ok(());
    }
    let why = format!(
        "Windows took {taken} of the {} key events: {}",
        strokes.len(),
        std::io::Error::last_os_error()
    );
    let letting_go = recovery(&releases(&strokes[..taken]));
    let held = still_held(&letting_go, send_now(&letting_go));
    if held.is_empty() {
        return Err(why);
    }
    Err(format!(
        "{why}. These keys may still be held down: {}",
        held.join(", ")
    ))
}

/// Give key events to SendInput in one call, and say how many it took.
fn send_now(strokes: &[Stroke]) -> usize {
    if strokes.is_empty() {
        return 0;
    }
    let inputs: Vec<INPUT> = strokes.iter().copied().map(input).collect();
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) as usize }
}

/// The batch that lets go of keys left down: a tap of [`BLANK`] first when
/// Win or Alt is among them, then the releases.
fn recovery(releases: &[Stroke]) -> Vec<Stroke> {
    let win_or_alt = releases.iter().any(|stroke| {
        matches!(
            key_name(stroke.key.code, stroke.key.extended),
            Some("left_command" | "right_command" | "left_option" | "right_option")
        )
    });
    let blank = win_or_alt.then_some([Stroke::down(BLANK), Stroke::up(BLANK)]);
    blank
        .into_iter()
        .flatten()
        .chain(releases.iter().copied())
        .collect()
}

/// The keys a recovery batch was to let go of, and did not, when Windows
/// took only `sent` of its events: by the labels the window shows for the
/// modifiers, and by name for any other key.
fn still_held(recovery: &[Stroke], sent: usize) -> Vec<String> {
    recovery[sent.min(recovery.len())..]
        .iter()
        .filter(|stroke| stroke.up && stroke.key != BLANK)
        .map(|stroke| {
            let name = key_name(stroke.key.code, stroke.key.extended).unwrap_or("a key");
            let modifier = name.strip_prefix("left_").unwrap_or(name);
            LABELS
                .iter()
                .find(|label| label.0 == modifier)
                .map_or(name, |label| label.1)
                .to_string()
        })
        .collect()
}

/// One key event as SendInput takes it, with the scan code and the extended
/// bit a real key would carry, which some apps read in place of the virtual
/// key. The scan code is the one the table gives, or else the one
/// `MapVirtualKeyW` gives. Windows gives an arrow the scan code of the same
/// key on the keypad, and the extended bit is what makes it the arrow. Every
/// event carries [`SENT_BY_BANDBRIDGE`], so the window's Record keys leaves
/// the keys this program sends alone.
fn input(stroke: Stroke) -> INPUT {
    let scan = match stroke.key.scan {
        0 => unsafe { MapVirtualKeyW(u32::from(stroke.key.code), MAPVK_VK_TO_VSC) },
        scan => u32::from(scan),
    };
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if stroke.key.extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if stroke.up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(stroke.key.code),
                wScan: (scan & 0xFF) as u16,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: SENT_BY_BANDBRIDGE,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{
        FLAG_COMMAND, FLAG_CONTROL, FLAG_FN, FLAG_OPTION, FLAG_SHIFT, KEYS, MEDIA_KEYS, names,
        sends_key, sends_media_key, sends_modifier,
    };
    use std::collections::{BTreeMap, BTreeSet};

    /// None of these tests sends a key. A test that pressed keys would type
    /// into whatever window is in front.
    fn down(name: &str) -> Stroke {
        Stroke::down(virtual_key(name).unwrap())
    }

    fn up(name: &str) -> Stroke {
        Stroke::up(virtual_key(name).unwrap())
    }

    // --- the names, and what Windows cannot send (W2) ------------------------

    /// Every key the Mac's table lists stands for a Windows virtual key, or
    /// is refused with why. The loader then refuses it by its name, and the
    /// window leaves it out.
    #[test]
    fn every_key_the_mac_lists_is_a_virtual_key_or_refused_with_why() {
        for (name, _) in KEYS {
            let sent = virtual_key(name).is_some();
            let why = unsupported_key(name);
            assert!(
                sent != why.is_some(),
                "{name} is {}",
                if sent {
                    "both sent and refused"
                } else {
                    "neither sent nor refused"
                }
            );
            assert_eq!(sends_key(name), sent, "{name}");
        }
        let refused: Vec<&str> = KEYS
            .iter()
            .map(|entry| entry.0)
            .filter(|name| !sends_key(name))
            .collect();
        assert_eq!(
            refused,
            [
                "help",
                "keypad_equal_sign",
                "keypad_comma",
                "fn",
                "japanese_eisuu",
                "japanese_kana",
                "international1",
                "international3"
            ]
        );
        assert_eq!(unsupported_key("fn"), Some(NO_FN));
        // Another spelling of a refused key is refused too.
        assert_eq!(unsupported_key("lang1"), unsupported_key("japanese_kana"));
        assert!(unsupported_key("lang1").is_some());
    }

    /// Each virtual key stands for one name the file takes, and names it
    /// back, so Record keys will name a key as the list does.
    #[test]
    fn every_virtual_key_names_its_key_back_once() {
        for (name, key) in VIRTUAL_KEYS {
            assert!(
                KEYS.iter().any(|entry| entry.0 == *name),
                "{name} is no key the file takes"
            );
            assert_eq!(key_name(key.code, key.extended), Some(*name), "{name}");
        }
        let distinct: BTreeSet<(u16, bool)> = every_key()
            .map(|(_, key)| (key.code, key.extended))
            .collect();
        assert_eq!(distinct.len(), every_key().count(), "two names share a key");

        // Keypad enter is return with the extended bit. An arrow reads the
        // same either way.
        assert_eq!(key_name(VK_RETURN.0, false), Some("return_or_enter"));
        assert_eq!(key_name(VK_RETURN.0, true), Some("keypad_enter"));
        assert_eq!(key_name(VK_UP.0, true), Some("up_arrow"));
        assert_eq!(key_name(VK_UP.0, false), Some("up_arrow"));
        // No name here for the help key, which no PC keyboard has, or for
        // the mail key.
        assert_eq!(key_name(VK_HELP.0, false), None);
        assert_eq!(heard_key(VK_LAUNCH_MAIL.0, true, 0), None);
    }

    /// Windows sends the keys a PC keyboard has beyond a Mac's (W2), and
    /// names each back, so Record keys will name them as the list does.
    #[test]
    fn the_keys_only_a_pc_has_are_sent_and_named_back() {
        let pc_keys = [
            ("insert", extended(VK_INSERT)),
            ("print_screen", scanned(extended(VK_SNAPSHOT), 0x37)),
            ("scroll_lock", vk(VK_SCROLL)),
            ("pause", scanned(vk(VK_PAUSE), 0x45)),
            ("application", extended(VK_APPS)),
            ("f21", vk(VK_F21)),
            ("f22", vk(VK_F22)),
            ("f23", vk(VK_F23)),
            ("f24", vk(VK_F24)),
        ];
        for (name, key) in pc_keys {
            // Name to key, and key to name.
            assert_eq!(virtual_key(name), Some(key), "{name}");
            assert_eq!(key_name(key.code, key.extended), Some(name), "{name}");
            assert!(sends_key(name), "{name}");
            assert_eq!(unsupported_key(name), None, "{name}");
            let heard = heard_key(key.code, key.extended, FLAG_SHIFT).unwrap();
            assert_eq!(heard.key_code.as_deref(), Some(name));
            assert_eq!(heard.modifiers, ["shift"]);
        }
        let names: Vec<&str> = pc_keys.iter().map(|entry| entry.0).collect();
        assert_eq!(keys_only_here().collect::<Vec<_>>(), names);
        assert!(
            !KEYS.iter().any(|entry| names.contains(&entry.0)),
            "the Mac lists one"
        );

        // Each carries the scan code and the extended bit a real key does,
        // Print Screen and Pause from the table.
        for (name, scan, extended_bit) in [
            ("insert", 0x52, true),
            ("print_screen", 0x37, true),
            ("scroll_lock", 0x46, false),
            ("pause", 0x45, false),
            ("application", 0x5D, true),
        ] {
            let key = virtual_key(name).unwrap();
            let bit = if extended_bit {
                KEYEVENTF_EXTENDEDKEY
            } else {
                KEYBD_EVENT_FLAGS(0)
            };
            let down = unsafe { input(Stroke::down(key)).Anonymous.ki };
            assert_eq!(
                (down.wVk.0, down.wScan, down.dwFlags),
                (key.code, scan, bit),
                "{name}"
            );
            let up = unsafe { input(Stroke::up(key)).Anonymous.ki };
            assert_eq!(
                (up.wScan, up.dwFlags),
                (scan, bit | KEYEVENTF_KEYUP),
                "{name}"
            );
        }
    }

    /// The keys a band button is most likely to press stand for the virtual
    /// keys Windows documents for them.
    #[test]
    fn maps_letters_digits_and_named_keys() {
        for (name, code) in [
            ("a", VK_A),
            ("z", VK_Z),
            ("0", VK_0),
            ("9", VK_9),
            ("return_or_enter", VK_RETURN),
            ("return", VK_RETURN),
            ("tab", VK_TAB),
            ("spacebar", VK_SPACE),
            ("escape", VK_ESCAPE),
            ("delete_or_backspace", VK_BACK),
            ("f1", VK_F1),
            ("f12", VK_F12),
            ("f13", VK_F13),
            ("f20", VK_F20),
            ("keypad_0", VK_NUMPAD0),
            ("hyphen", VK_OEM_MINUS),
            ("grave_accent_and_tilde", VK_OEM_3),
            ("non_us_backslash", VK_OEM_102),
            ("caps_lock", VK_CAPITAL),
            ("left_shift", VK_LSHIFT),
            ("left_option", VK_LMENU),
            ("left_control", VK_LCONTROL),
            ("right_shift", VK_RSHIFT),
        ] {
            assert_eq!(virtual_key(name), Some(vk(code)), "{name}");
        }
        // The keys a real keyboard sends with the extended bit.
        for (name, code) in [
            ("up_arrow", VK_UP),
            ("left_arrow", VK_LEFT),
            ("home", VK_HOME),
            ("page_up", VK_PRIOR),
            ("prior", VK_PRIOR),
            ("page_down", VK_NEXT),
            ("delete_forward", VK_DELETE),
            ("keypad_enter", VK_RETURN),
            ("keypad_slash", VK_DIVIDE),
            ("keypad_num_lock", VK_NUMLOCK),
            ("clear", VK_NUMLOCK),
            ("left_command", VK_LWIN),
            ("left_gui", VK_LWIN),
            ("right_command", VK_RWIN),
            ("right_alt", VK_RMENU),
            ("right_control", VK_RCONTROL),
        ] {
            assert_eq!(virtual_key(name), Some(extended(code)), "{name}");
        }
        assert_eq!(virtual_key("meta"), None);
    }

    /// The media keys a PC keyboard has go out as Windows media keys, and
    /// the rest are refused with why.
    #[test]
    fn the_media_keys_are_windows_media_keys_or_refused() {
        for (name, _) in MEDIA_KEYS {
            let sent = media_virtual_key(name).is_some();
            assert!(sent != unsupported_media_key(name).is_some(), "{name}");
            assert_eq!(sends_media_key(name), sent, "{name}");
        }
        for (name, code) in [
            ("volume_increment", VK_VOLUME_UP),
            ("volume_decrement", VK_VOLUME_DOWN),
            ("mute", VK_VOLUME_MUTE),
            ("play_or_pause", VK_MEDIA_PLAY_PAUSE),
            ("scan_next_track", VK_MEDIA_NEXT_TRACK),
            ("scan_previous_track", VK_MEDIA_PREV_TRACK),
        ] {
            assert_eq!(media_virtual_key(name), Some(code.0), "{name}");
            assert_eq!(media_key_name(code.0), Some(name));
            let heard = heard_media_key(code.0).unwrap();
            assert_eq!(heard.consumer_key_code.as_deref(), Some(name));
            assert!(heard.key_code.is_none() && heard.modifiers.is_empty());
        }
        let refused: Vec<&str> = MEDIA_KEYS
            .iter()
            .map(|entry| entry.0)
            .filter(|name| !sends_media_key(name))
            .collect();
        assert_eq!(
            refused,
            [
                "fast_forward",
                "rewind",
                "eject",
                "display_brightness_increment",
                "display_brightness_decrement",
                "illumination_increment",
                "illumination_decrement"
            ]
        );
        assert_eq!(
            unsupported_media_key("display_brightness_increment"),
            Some(NO_BRIGHTNESS)
        );
        assert_eq!(
            unsupported_media_key("fastforward"),
            unsupported_media_key("fast_forward")
        );
    }

    /// Command is the Win key and option is Alt. The window labels them
    /// Ctrl, Alt, Shift and Win, and Windows has no fn to hold.
    #[test]
    fn command_is_the_win_key_and_option_is_alt() {
        let held: Vec<(u64, Option<VirtualKey>)> = MODIFIER_KEYS
            .iter()
            .map(|&(flag, name)| (flag, virtual_key(name)))
            .collect();
        assert_eq!(
            held,
            [
                (FLAG_CONTROL, Some(vk(VK_LCONTROL))),
                (FLAG_OPTION, Some(vk(VK_LMENU))),
                (FLAG_SHIFT, Some(vk(VK_LSHIFT))),
                (FLAG_COMMAND, Some(extended(VK_LWIN)))
            ]
        );
        for name in [
            "command",
            "left_command",
            "cmd",
            "shift",
            "option",
            "alt",
            "right_alt",
            "control",
            "ctrl",
        ] {
            assert!(sends_modifier(name), "{name}");
            assert_eq!(unsupported_modifier(name), None, "{name}");
        }
        for name in ["fn", "left_fn", "right_fn"] {
            assert!(!sends_modifier(name), "{name}");
            assert_eq!(unsupported_modifier(name), Some(NO_FN), "{name}");
        }
        let labels: BTreeMap<&str, &str> = LABELS.iter().copied().collect();
        assert_eq!(
            labels,
            BTreeMap::from([
                ("command", "Win"),
                ("control", "Ctrl"),
                ("option", "Alt"),
                ("shift", "Shift")
            ])
        );
    }

    /// A Karabiner name the Mac's table does not list is refused with words
    /// that are true on Windows, not the Mac's own.
    #[test]
    fn a_name_the_list_lacks_is_refused_in_words_true_on_windows() {
        for name in ["undo", "copy", "menu", "power", "lang3", "japanese_pc_xfer"] {
            assert_eq!(unsupported_key(name), Some(NOT_SENT), "{name}");
            assert!(!sends_key(name), "{name}");
        }
        assert_eq!(NOT_SENT, "Choose a key from the list in the window.");
        let feature = unsupported_key("mission_control").unwrap();
        assert!(feature.contains("macOS feature"), "{feature}");
        assert_eq!(
            unsupported_key("volume_up"),
            Some("This is a media key. Set consumer_key_code = \"volume_increment\".")
        );
        assert_eq!(
            unsupported_key("apple_display_brightness_increment"),
            Some(NO_BRIGHTNESS)
        );
        assert_eq!(
            unsupported_key("meta"),
            None,
            "no Karabiner name, so unknown"
        );
    }

    /// The window lists only what Windows sends, with the Windows labels.
    #[test]
    fn the_window_lists_what_windows_sends_with_windows_labels() {
        let json = serde_json::to_value(names()).unwrap();
        // The Mac's 116, less the 8 Windows cannot send, and the PC's 9 last.
        let keys = json["keys"].as_array().unwrap();
        assert_eq!(keys.len(), 116 - 8 + 9);
        assert_eq!(
            keys[keys.len() - 9..],
            serde_json::json!([
                "insert",
                "print_screen",
                "scroll_lock",
                "pause",
                "application",
                "f21",
                "f22",
                "f23",
                "f24"
            ])
            .as_array()
            .unwrap()[..]
        );
        assert_eq!(
            json["mediaKeys"],
            serde_json::json!([
                "volume_increment",
                "volume_decrement",
                "mute",
                "play_or_pause",
                "scan_next_track",
                "scan_previous_track"
            ])
        );
        assert_eq!(
            json["modifiers"],
            serde_json::json!(["command", "shift", "option", "control"])
        );
        // lang1 and lang2 are Japanese keys, and fast_forward is refused.
        assert_eq!(json["aliases"]["keys"].as_object().unwrap().len(), 9 - 2);
        assert_eq!(json["aliases"]["mediaKeys"], serde_json::json!({}));
        // left_fn and right_fn are gone.
        assert_eq!(
            json["aliases"]["modifiers"].as_object().unwrap().len(),
            19 - 2
        );
        assert_eq!(
            json["labels"],
            serde_json::json!({"command": "Win", "control": "Ctrl", "option": "Alt", "shift": "Shift"})
        );
    }

    // --- naming what a person pressed, for Record keys -----------------------

    /// The modifiers come from the flags, by the names the list gives them,
    /// in its order, and never fn.
    #[test]
    fn a_heard_key_holds_the_modifiers_its_flags_hold() {
        let heard = heard_key(VK_K.0, false, FLAG_COMMAND | FLAG_SHIFT | FLAG_FN).unwrap();
        assert_eq!(heard.key_code.as_deref(), Some("k"));
        assert_eq!(heard.modifiers, ["command", "shift"]);
        assert!(heard.consumer_key_code.is_none());
        let plain = heard_key(VK_F13.0, false, 0).unwrap();
        assert_eq!(
            (plain.key_code.as_deref(), plain.modifiers.len()),
            (Some("f13"), 0)
        );
        assert_eq!(
            heard_key(VK_RETURN.0, true, FLAG_CONTROL).map(|k| (k.key_code, k.modifiers)),
            Some((
                Some("keypad_enter".to_string()),
                vec!["control".to_string()]
            ))
        );
    }

    // --- one batch, and no key left held (Review Focus 2) --------------------

    /// A keystroke presses its modifiers in the Mac's order, then the key,
    /// and lets the modifiers go in the reverse order.
    #[test]
    fn a_keystroke_holds_its_modifiers_around_the_key() {
        let k = virtual_key("k").unwrap();
        assert_eq!(
            strokes(k, FLAG_COMMAND | FLAG_SHIFT | FLAG_OPTION | FLAG_CONTROL),
            [
                down("left_control"),
                down("left_option"),
                down("left_shift"),
                down("left_command"),
                Stroke::down(k),
                Stroke::up(k),
                up("left_command"),
                up("left_shift"),
                up("left_option"),
                up("left_control"),
            ]
        );
        assert_eq!(strokes(k, 0), [Stroke::down(k), Stroke::up(k)]);
        // Windows has no fn key, so fn adds nothing.
        assert_eq!(strokes(k, FLAG_FN), strokes(k, 0));
    }

    /// Whatever part of a batch Windows takes, the releases let go of every
    /// key that part pressed, the last pressed first, and leave none held.
    #[test]
    fn a_batch_taken_in_part_leaves_no_key_held() {
        let k = virtual_key("k").unwrap();
        let all = strokes(k, FLAG_COMMAND | FLAG_SHIFT | FLAG_CONTROL);
        for taken in 0..=all.len() {
            let mut sent = all[..taken].to_vec();
            let letting_go = releases(&sent);
            assert!(
                letting_go.iter().all(|stroke| stroke.up),
                "{taken}: {letting_go:?}"
            );
            sent.extend(letting_go);
            assert!(
                releases(&sent).is_empty(),
                "after {taken} of {} events, a key stays held",
                all.len()
            );
        }
        // Taken up to the key's own press: every key so far goes up, the key
        // first.
        assert_eq!(
            releases(&all[..4]),
            [
                Stroke::up(k),
                up("left_command"),
                up("left_shift"),
                up("left_control")
            ]
        );
        assert!(releases(&all).is_empty(), "a whole batch holds nothing");
    }

    /// The batch that lets go of keys left down taps a key no key has before
    /// it releases Win or Alt, so neither opens Start or a menu bar. It needs
    /// no tap for the others.
    #[test]
    fn letting_go_of_win_or_alt_opens_nothing() {
        let k = virtual_key("k").unwrap();
        let win = [Stroke::up(k), up("left_command"), up("left_shift")];
        assert_eq!(
            recovery(&win),
            [
                Stroke::down(BLANK),
                Stroke::up(BLANK),
                Stroke::up(k),
                up("left_command"),
                up("left_shift")
            ]
        );
        let alt = [up("left_option")];
        assert_eq!(
            recovery(&alt)[..2],
            [Stroke::down(BLANK), Stroke::up(BLANK)]
        );
        let plain = [Stroke::up(k), up("left_shift"), up("left_control")];
        assert_eq!(recovery(&plain), plain);
        assert!(recovery(&[]).is_empty());
        assert_eq!(key_name(BLANK.code, false), None, "the blank is some key");
    }

    /// When Windows does not take all of that batch either, the error names
    /// each key that may still be down, as the window labels it.
    #[test]
    fn a_release_taken_in_part_names_the_keys_left_down() {
        let k = virtual_key("k").unwrap();
        let letting_go = recovery(&[Stroke::up(k), up("left_command"), up("left_shift")]);
        assert_eq!(still_held(&letting_go, 3), ["Win", "Shift"]);
        assert_eq!(still_held(&letting_go, 0), ["k", "Win", "Shift"]);
        assert_eq!(
            still_held(&letting_go, 1),
            ["k", "Win", "Shift"],
            "the blank is no key"
        );
        assert!(still_held(&letting_go, letting_go.len()).is_empty());
        let plain = recovery(&[up("left_option"), up("left_control")]);
        assert_eq!(still_held(&plain, 2), ["Alt", "Ctrl"]);
    }

    /// A key event carries the scan code a real key would, and the extended
    /// bit on the keys that have it: an arrow, and keypad enter.
    #[test]
    fn a_key_event_looks_like_a_real_key() {
        let event = |name: &str, up: bool| {
            let input = input(Stroke {
                key: virtual_key(name).unwrap(),
                up,
            });
            assert_eq!(input.r#type, INPUT_KEYBOARD);
            unsafe { input.Anonymous.ki }
        };
        let arrow = event("up_arrow", false);
        assert_eq!((arrow.wVk, arrow.wScan), (VK_UP, 0x48));
        assert_eq!(arrow.dwFlags, KEYEVENTF_EXTENDEDKEY);
        let enter = event("keypad_enter", true);
        assert_eq!((enter.wVk, enter.wScan), (VK_RETURN, 0x1C));
        assert_eq!(enter.dwFlags, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP);
        let plain = event("return_or_enter", false);
        assert_eq!((plain.wVk, plain.wScan), (VK_RETURN, 0x1C));
        assert_eq!(plain.dwFlags, KEYBD_EVENT_FLAGS(0));
        let win = event("left_command", true);
        assert_eq!((win.wVk, win.wScan), (VK_LWIN, 0x5B));
        assert_eq!(win.dwFlags, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP);
        let shift = event("left_shift", false);
        assert_eq!(
            (shift.wVk, shift.wScan, shift.dwFlags),
            (VK_LSHIFT, 0x2A, KEYBD_EVENT_FLAGS(0))
        );
    }

    /// Every key event this program sends carries its mark, the blank key's
    /// included, so the window's Record keys leaves them alone. A key a person
    /// presses carries none. The mark is not zero, which is what a keyboard
    /// sends.
    #[test]
    fn every_key_event_carries_bandbridges_mark() {
        let marked = |stroke: Stroke| unsafe { input(stroke).Anonymous.ki.dwExtraInfo };
        for name in ["k", "up_arrow", "left_command", "print_screen", "f24"] {
            let key = virtual_key(name).unwrap();
            assert_eq!(marked(Stroke::down(key)), SENT_BY_BANDBRIDGE, "{name}");
            assert_eq!(marked(Stroke::up(key)), SENT_BY_BANDBRIDGE, "{name}");
        }
        assert_eq!(marked(Stroke::down(BLANK)), SENT_BY_BANDBRIDGE);
        let media = VirtualKey {
            code: VK_VOLUME_UP.0,
            extended: true,
            scan: 0,
        };
        assert_eq!(marked(Stroke::up(media)), SENT_BY_BANDBRIDGE);
        assert_ne!(SENT_BY_BANDBRIDGE, 0, "zero is what a keyboard sends");
    }

    /// Windows asks no permission to send keys.
    #[test]
    fn sending_keys_needs_no_permission() {
        assert!(trusted(false));
        assert!(trusted(true));
    }
}
