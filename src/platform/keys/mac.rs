//! Posting keys on macOS, and naming the keys the window's Record keys hears.
//!
//! macOS uses two different APIs. An ordinary key is a CGEvent carrying a
//! virtual keycode. A media key is an NSEvent system-defined event, which is
//! why the two are kept apart all the way down rather than unified early.
//!
//! The codes are the ones in the tables of [`crate::keys`], which are
//! macOS's own.

use crate::config::Keystroke;
use crate::keys::{
    FLAG_COMMAND, FLAG_CONTROL, FLAG_FN, FLAG_OPTION, FLAG_SHIFT, KEYS, MEDIA_KEYS, MODIFIERS,
    consumer_code, key_code, modifier_flag,
};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
use objc2_core_graphics::{CGEvent, CGEventFlags, CGEventTapLocation};
use objc2_foundation::{NSDictionary, NSNumber, NSPoint, NSString};
use objc2::rc::Retained;

/// The Mac's window draws its own marks for the modifiers, ⌘ ⇧ ⌥ ⌃, so it
/// takes no labels from here.
pub const LABELS: &[(&str, &str)] = &[];

/// The name [`KEYS`] lists a macOS keycode under: [`key_code`] the other way.
pub fn key_name(code: u16) -> Option<&'static str> {
    KEYS.iter()
        .find(|entry| entry.1 == code)
        .map(|entry| entry.0)
}

/// The name [`MEDIA_KEYS`] lists an NX_KEYTYPE under: [`consumer_code`] the
/// other way.
pub fn media_key_name(code: u16) -> Option<&'static str> {
    MEDIA_KEYS
        .iter()
        .find(|entry| u16::from(entry.1) == code)
        .map(|entry| entry.0)
}

/// The keystroke a key a person pressed stands for, as the window's Record
/// keys names it: the key by the name [`KEYS`] gives its keycode, and the
/// modifiers the event's flags hold, in the order [`MODIFIERS`] lists them.
/// None for a keycode this program has no name for, and so cannot post.
///
/// A keyboard sets fn on the function keys, the arrows and the navigation
/// cluster by itself, whether or not fn is held, so fn is left out for them.
/// Kept, it would be pressed each time the action fires. Caps lock and the
/// numeric pad flag are no modifiers a keystroke holds.
pub fn heard_key(code: u16, flags: u64) -> Option<Keystroke> {
    let key = key_name(code)?;
    let modifiers = MODIFIERS
        .iter()
        .filter(|(_, flag)| flags & flag != 0)
        .filter(|(_, flag)| !(*flag == FLAG_FN && sets_fn_itself(key)))
        .map(|(modifier, _)| modifier.to_string())
        .collect();
    Some(Keystroke {
        key_code: Some(key.to_string()),
        consumer_key_code: None,
        modifiers,
    })
}

/// The keystroke a media key a person pressed stands for: its name from
/// [`MEDIA_KEYS`], and no modifiers, which a media key never takes. None for
/// an NX_KEYTYPE this program cannot post.
pub fn heard_media_key(code: u16) -> Option<Keystroke> {
    Some(Keystroke {
        key_code: None,
        consumer_key_code: Some(media_key_name(code)?.to_string()),
        modifiers: Vec::new(),
    })
}

/// Whether a keyboard sets fn on this key by itself: a function key, an
/// arrow, or a key of the navigation cluster. AppKit calls them all function
/// keys, and reports fn on them.
fn sets_fn_itself(key: &str) -> bool {
    let function_key = key
        .strip_prefix('f')
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()));
    function_key
        || key.ends_with("_arrow")
        || matches!(
            key,
            "home" | "end" | "page_up" | "page_down" | "delete_forward" | "help"
        )
}

/// Why a Karabiner key name cannot be sent from here.
///
/// Karabiner reaches keys through a driver of its own. This program posts
/// CGEvents, which carry a macOS keycode, and macOS has no keycode for some of
/// what Karabiner names. Saying which is which turns a dead binding into a
/// message a person can act on.
pub fn unsupported_key(name: &str) -> Option<&'static str> {
    let reason = match name {
        "f21" | "f22" | "f23" | "f24" => "macOS stops at f20.",

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
        "rewind" => "This is a media key. Set consumer_key_code = \"rewind\".",
        "fastforward" => "This is a media key. Set consumer_key_code = \"fast_forward\".",
        "display_brightness_increment"
        | "apple_display_brightness_increment"
        | "apple_top_case_display_brightness_increment"
        | "vk_consumer_brightness_up" => {
            "This is a media key. Set consumer_key_code = \"display_brightness_increment\"."
        }
        "display_brightness_decrement"
        | "apple_display_brightness_decrement"
        | "apple_top_case_display_brightness_decrement"
        | "vk_consumer_brightness_down" => {
            "This is a media key. Set consumer_key_code = \"display_brightness_decrement\"."
        }
        "illumination_increment" | "vk_consumer_illumination_up" => {
            "This is a media key. Set consumer_key_code = \"illumination_increment\"."
        }
        "illumination_decrement" | "vk_consumer_illumination_down" => {
            "This is a media key. Set consumer_key_code = \"illumination_decrement\"."
        }

        "mission_control" | "vk_mission_control" | "launchpad" | "vk_launchpad"
        | "dashboard" | "vk_dashboard" => {
            "macOS has no keycode for this. Karabiner opens it through its own driver."
        }

        "insert" => "macOS has no insert key. The key in that place is help.",
        "print_screen" => "macOS has no print screen key. The key in that place is f13.",
        "scroll_lock" => "macOS has no scroll lock key. The key in that place is f14.",
        "pause" => "macOS has no pause key. The key in that place is f15.",

        "again" | "alternate_erase" | "application" | "cancel" | "clear_or_again"
        | "copy" | "cr_sel_or_props" | "cut" | "execute" | "ex_sel" | "find"
        | "keypad_equal_sign_as400" | "locking_caps_lock" | "locking_num_lock"
        | "locking_scroll_lock" | "menu" | "non_us_pound" | "oper" | "out"
        | "paste" | "power" | "select" | "separator" | "stop"
        | "sys_req_or_attention" | "undo" => {
            "a Mac keyboard has no such key, and macOS has no keycode for it."
        }

        "international2" | "international4" | "international5" | "international6"
        | "international7" | "international8" | "international9" | "lang3"
        | "lang4" | "lang5" | "lang6" | "lang7" | "lang8" | "lang9"
        | "japanese_pc_katakana" | "japanese_pc_nfer" | "japanese_pc_xfer" => {
            "macOS has no keycode for this key."
        }

        "vk_none" => "this disables a key in Karabiner. It sends nothing.",

        _ => return None,
    };
    Some(reason)
}

/// macOS sends every media key [`MEDIA_KEYS`] lists, so none is refused.
pub fn unsupported_media_key(_name: &str) -> Option<&'static str> {
    None
}

/// macOS holds every modifier [`MODIFIERS`] lists, so none is refused.
pub fn unsupported_modifier(_name: &str) -> Option<&'static str> {
    None
}

/// macOS sends only the keys [`KEYS`] lists, so it has none of its own.
pub fn keys_only_here() -> impl Iterator<Item = &'static str> {
    std::iter::empty()
}

// Whether this process may post input events.
//
// Without this, CGEvent::post silently does nothing: no error, no keystroke.
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const NSDictionary<NSString, NSNumber>) -> bool;
    /// The one option key: show the system dialog when the answer is no.
    /// CFString and NSString are the same object, so the type is safe here.
    static kAXTrustedCheckOptionPrompt: &'static NSString;
}

/// Whether this process may post input events.
///
/// `prompt` lets macOS show its permission dialog when the answer is no. Only
/// pass true when the user asked for it. macOS shows the dialog on every
/// failing call, so a start-up check must never prompt: a program that cannot
/// get the permission would then pop a dialog on every run.
pub fn trusted(prompt: bool) -> bool {
    if !prompt {
        return unsafe { AXIsProcessTrusted() };
    }
    let key: &NSString = unsafe { kAXTrustedCheckOptionPrompt };
    let yes = NSNumber::new_bool(true);
    let options = NSDictionary::from_slices(&[key], &[&*yes]);
    unsafe { AXIsProcessTrustedWithOptions(Retained::as_ptr(&options)) }
}

/// Post one ordinary keystroke.
///
/// Down and up are posted together with nothing fallible between them. A
/// modifier left logically down is held system-wide until the user logs out.
/// The virtual key code for a modifier, so it can be pressed rather than only
/// declared.
///
/// Setting the flag on a key event is not the same as holding the key. Some
/// shortcuts watch for the modifier going down and never see one, so they do
/// not fire, while the same keys pressed by hand work.
fn modifier_key_code(flag: u64) -> Option<u16> {
    match flag {
        FLAG_COMMAND => Some(0x37),
        FLAG_SHIFT => Some(0x38),
        FLAG_OPTION => Some(0x3A),
        FLAG_CONTROL => Some(0x3B),
        FLAG_FN => Some(0x3F),
        _ => None,
    }
}

/// The modifiers in a flag set, in the order they should be pressed.
fn modifier_flags(flags: u64) -> Vec<u64> {
    [FLAG_CONTROL, FLAG_OPTION, FLAG_SHIFT, FLAG_COMMAND, FLAG_FN]
        .into_iter()
        .filter(|flag| flags & flag != 0)
        .collect()
}

/// Post one keystroke, holding its modifiers down around it.
///
/// Every event is built before any is posted. A failure halfway would otherwise
/// leave a modifier held down system-wide, which lasts until the user logs out.
/// An event that cannot be built is the error, and nothing is posted.
pub fn tap_key(code: u16, flags: u64) -> Result<(), String> {
    let mut events: Vec<(u16, bool, u64)> = Vec::new();

    // Press the modifiers, building up the flag set as each goes down, the way
    // a keyboard reports it.
    let mut held = 0u64;
    for flag in modifier_flags(flags) {
        let Some(key) = modifier_key_code(flag) else {
            continue;
        };
        held |= flag;
        events.push((key, true, held));
    }

    events.push((code, true, flags));
    events.push((code, false, flags));

    // Release them in the reverse order, taking each flag off as it goes up.
    for flag in modifier_flags(flags).into_iter().rev() {
        let Some(key) = modifier_key_code(flag) else {
            continue;
        };
        held &= !flag;
        events.push((key, false, held));
    }

    let mut built = Vec::with_capacity(events.len());
    for (key, down, state) in events {
        let Some(event) = CGEvent::new_keyboard_event(None, key, down) else {
            // Post nothing rather than half a keypress.
            return Err(format!("macOS could not make a key event for code {key}"));
        };
        CGEvent::set_flags(Some(&event), CGEventFlags(state));
        built.push(event);
    }
    for event in built {
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
    }
    Ok(())
}

/// Post one media key.
///
/// Both events are built before either is posted, so a failure cannot leave
/// the key logically held. This mirrors tap_key deliberately.
///
/// Media keys are not CGEvent keyboard events. They are NSEvent system-defined
/// events with subtype 8, carrying the key and its state packed into data1.
pub fn tap_media(code: u8) -> Result<(), String> {
    // Build both events before posting either. Posting a down event and then
    // failing to build the up event would leave the media key logically held.
    // A closure rather than a named fn: the value is `Retained<CGEvent>`, and
    // `Retained` lives in `objc2`, a transitive dependency only, so a named fn
    // would have to write the return type as `impl Deref<Target = CGEvent>`.
    let media_event = |down: bool| {
        let state: i32 = if down { 0xa } else { 0xb };
        let data1 = (((code as i32) << 16) | (state << 8)) as isize;
        let flags = NSEventModifierFlags(if down { 0xa00 } else { 0xb00 });
        NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
            NSEventType::SystemDefined,
            NSPoint::new(0.0, 0.0),
            flags,
            0.0,
            0,
            None,
            8,
            data1,
            -1,
        )?
        .CGEvent()
    };

    let (Some(down), Some(up)) = (media_event(true), media_event(false)) else {
        return Err(format!(
            "macOS could not make a media key event for code {code}"
        ));
    };
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&down));
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&up));
    Ok(())
}

/// Fire a checked keystroke. The loader checks every name, so an unknown one
/// here means the keystroke never went through it, and nothing is posted.
pub fn fire(keystroke: &Keystroke) -> Result<(), String> {
    if let Some(name) = &keystroke.consumer_key_code {
        let code =
            consumer_code(name).ok_or_else(|| format!("unknown consumer_key_code {name}"))?;
        return tap_media(code);
    }
    let Some(name) = &keystroke.key_code else {
        return Err("a keystroke needs key_code or consumer_key_code".to_string());
    };
    let code = key_code(name).ok_or_else(|| format!("unknown key_code {name}"))?;
    let flags = keystroke
        .modifiers
        .iter()
        .filter_map(|m| modifier_flag(m))
        .fold(0u64, |acc, f| acc | f);
    tap_key(code, flags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::names;

    /// A shortcut watches the modifier go down, so the sequence must press it
    /// and release it, not merely declare it on the key event.
    #[test]
    fn a_modifier_is_pressed_and_released_around_the_key() {
        let flags = modifier_flags(FLAG_COMMAND | FLAG_CONTROL);
        // Control first, command last, which is the order a keyboard reports.
        assert_eq!(flags, vec![FLAG_CONTROL, FLAG_COMMAND]);
        assert_eq!(modifier_key_code(FLAG_COMMAND), Some(0x37));
        assert_eq!(modifier_key_code(FLAG_CONTROL), Some(0x3B));
        assert_eq!(modifier_key_code(0), None);
    }

    /// The Mac says why a name cannot be sent in its own words, and refuses
    /// no media key or modifier it lists.
    #[test]
    fn the_mac_says_why_in_its_own_words() {
        assert_eq!(unsupported_key("f21"), Some("macOS stops at f20."));
        assert_eq!(
            unsupported_key("insert"),
            Some("macOS has no insert key. The key in that place is help.")
        );
        assert_eq!(unsupported_media_key("display_brightness_increment"), None);
        assert_eq!(unsupported_modifier("fn"), None);
        assert!(LABELS.is_empty());
    }

    /// With no modifiers there is nothing to hold, and the key stands alone.
    #[test]
    fn a_plain_key_holds_nothing() {
        assert!(modifier_flags(0).is_empty());
    }

    // --- naming what a person pressed, for Record keys ------------------------

    /// Every key and media key the list offers is named back from its code,
    /// so whatever Record keys hears, it names as the list does.
    #[test]
    fn every_code_is_named_back_as_the_list_names_it() {
        let names = names();
        for key in &names.keys {
            let code = key_code(key).unwrap();
            assert_eq!(key_name(code), Some(*key), "keycode {code}");
        }
        for media in &names.media_keys {
            let code = consumer_code(media).unwrap();
            assert_eq!(
                media_key_name(u16::from(code)),
                Some(*media),
                "NX code {code}"
            );
        }
        // Codes with no name here: keycode 72 is volume up on an old
        // keyboard, and NX code 4 is caps lock.
        assert_eq!(key_name(72), None);
        assert_eq!(heard_key(72, 0), None);
        assert_eq!(media_key_name(4), None);
        assert_eq!(heard_media_key(4), None);
    }

    fn keystroke(key: &str, modifiers: &[&str]) -> Keystroke {
        Keystroke {
            key_code: Some(key.to_string()),
            consumer_key_code: None,
            modifiers: modifiers.iter().map(|m| m.to_string()).collect(),
        }
    }

    /// The modifiers come from the key's flags, by the names the list gives
    /// them, in its order, whatever order they were pressed in.
    #[test]
    fn a_heard_key_holds_the_modifiers_its_flags_hold() {
        assert_eq!(heard_key(40, 0), Some(keystroke("k", &[])));
        assert_eq!(
            heard_key(21, FLAG_SHIFT | FLAG_COMMAND),
            Some(keystroke("4", &["command", "shift"]))
        );
        assert_eq!(
            heard_key(40, FLAG_CONTROL | FLAG_OPTION | FLAG_SHIFT | FLAG_COMMAND),
            Some(keystroke("k", &["command", "shift", "option", "control"]))
        );
        // Caps lock (0x10000) and the numeric pad flag (0x200000) are no
        // modifiers a keystroke holds.
        assert_eq!(
            heard_key(40, 0x0001_0000 | 0x0020_0000),
            Some(keystroke("k", &[]))
        );
        // What a heard key names, the file takes.
        let heard = heard_key(21, FLAG_SHIFT | FLAG_COMMAND).unwrap();
        for modifier in &heard.modifiers {
            assert!(modifier_flag(modifier).is_some(), "{modifier}");
        }
    }

    /// A keyboard sets fn on the function keys, the arrows and the
    /// navigation cluster by itself. Fn from their flags is never recorded,
    /// and fn held with any other key is.
    #[test]
    fn fn_from_a_function_arrow_or_navigation_key_is_never_recorded() {
        let numeric_pad = 0x0020_0000;
        for (code, name) in [
            (96, "f5"),
            (122, "f1"),
            (90, "f20"),
            (126, "up_arrow"),
            (123, "left_arrow"),
            (115, "home"),
            (119, "end"),
            (116, "page_up"),
            (121, "page_down"),
            (117, "delete_forward"),
            (114, "help"),
        ] {
            assert_eq!(
                heard_key(code, FLAG_FN | numeric_pad),
                Some(keystroke(name, &[])),
                "{name}"
            );
            assert_eq!(
                heard_key(code, FLAG_FN | FLAG_COMMAND),
                Some(keystroke(name, &["command"])),
                "{name} keeps the other modifiers"
            );
        }
        assert_eq!(heard_key(14, FLAG_FN), Some(keystroke("e", &["fn"])));
        assert_eq!(heard_key(29, FLAG_FN), Some(keystroke("0", &["fn"])));
        // keypad_1 is no function key, whatever its name starts with.
        assert_eq!(heard_key(83, FLAG_FN), Some(keystroke("keypad_1", &["fn"])));
    }

    /// A media key is a consumer key, with no modifiers.
    #[test]
    fn a_heard_media_key_takes_no_modifiers() {
        assert_eq!(
            heard_media_key(0),
            Some(Keystroke {
                key_code: None,
                consumer_key_code: Some("volume_increment".to_string()),
                modifiers: Vec::new(),
            })
        );
        assert_eq!(
            heard_media_key(16).and_then(|k| k.consumer_key_code),
            Some("play_or_pause".to_string())
        );
        let names = names();
        assert_eq!(names.media_keys.len(), 13);
        for media in names.media_keys {
            let code = u16::from(consumer_code(media).unwrap());
            let heard = heard_media_key(code).unwrap();
            assert_eq!(heard.consumer_key_code.as_deref(), Some(media));
            assert!(heard.modifiers.is_empty());
        }
    }

    /// The window gets three groups, and the other spellings of each.
    #[test]
    fn the_key_names_reach_the_window_in_three_groups() {
        let json = serde_json::to_value(names()).unwrap();
        assert_eq!(json["keys"].as_array().unwrap().len(), 116);
        assert_eq!(json["keys"][0], "a");
        assert_eq!(json["mediaKeys"].as_array().unwrap().len(), 13);
        assert_eq!(json["mediaKeys"][0], "volume_increment");
        assert_eq!(
            json["modifiers"],
            serde_json::json!(["command", "shift", "option", "control", "fn"])
        );
        assert_eq!(json["aliases"]["keys"].as_object().unwrap().len(), 9);
        assert_eq!(json["aliases"]["keys"]["return"], "return_or_enter");
        assert_eq!(json["aliases"]["keys"]["left_gui"], "left_command");
        assert_eq!(
            json["aliases"]["mediaKeys"],
            serde_json::json!({"fastforward": "fast_forward"})
        );
        assert_eq!(json["aliases"]["modifiers"].as_object().unwrap().len(), 19);
        assert_eq!(json["aliases"]["modifiers"]["left_command"], "command");
        assert_eq!(json["aliases"]["modifiers"]["right_alt"], "option");
        assert_eq!(json["aliases"]["modifiers"]["ctrl"], "control");
        // The Mac's window draws its own marks, so the page gets no labels,
        // and the JSON is what it was before Windows.
        assert_eq!(json.get("labels"), None);
    }
}
