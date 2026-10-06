//! Names for the band's built-in screens.
//!
//! The band sends ids, never names. These tables come from Gadgetbridge's
//! ZeppOsMenuType, so the names match what other tools show. An id with no
//! entry here is printed as itself, which is what an installed app looks like.

/// Menu entries: the app list reached from the watch face.
pub const MENU: &[(&str, &str)] = &[
    ("00000001", "personal_activity_intelligence"),
    ("00000002", "hr"),
    ("00000003", "workout"),
    ("00000004", "weather"),
    ("00000009", "alarm"),
    ("0000000A", "takephoto"),
    ("0000000B", "music"),
    ("0000000C", "stopwatch"),
    ("0000000D", "countdown"),
    ("0000000E", "findphone"),
    ("0000000F", "mutephone"),
    ("00000010", "cards"),
    ("00000011", "alipay"),
    ("00000013", "settings"),
    ("00000014", "workout_history"),
    ("00000015", "eventreminder"),
    ("00000016", "compass"),
    ("00000019", "pai"),
    ("0000001A", "worldclock"),
    ("0000001C", "stress"),
    ("0000001D", "female_health"),
    ("0000001E", "workout_status"),
    ("00000020", "calendar"),
    ("00000023", "sleep"),
    ("00000024", "spo2"),
    ("00000025", "phone"),
    ("00000026", "events"),
    ("00000031", "wechat_pay"),
    ("00000033", "breathing"),
    ("00000038", "pomodoro"),
    ("00000039", "alexa2"),
    ("0000003B", "thermometer"),
    ("0000003E", "todo"),
    ("0000003F", "mi_ai"),
    ("00000041", "barometer"),
    ("00000042", "voice_memos"),
    ("00000044", "sun_moon"),
    ("00000045", "one_tap_measuring"),
    ("00000046", "zepp_coach"),
    ("00000047", "membership_cards"),
    ("00000049", "body_composition"),
    ("0000004A", "readiness"),
    ("0000004B", "map"),
    ("0000004C", "zepp_pay"),
    ("0000004D", "heart_rate_push"),
    ("00000052", "audio_control"),
    ("00000053", "golf"),
    ("00000056", "bio_charge"),
    ("00000057", "training_calendar"),
    ("00000100", "alexa"),
    ("00000101", "offline_voice"),
    ("00000102", "flashlight"),
    ("000F4258", "real_time_heart_rate"),
    ("000F4259", "water_time"),
    ("000F425B", "calculator"),
    ("000F4263", "watch_storage_space"),
    ("000F5D0A", "running_calculator"),
    ("000F636F", "diary"),
    ("000F63A5", "calorie_intake_record"),
    ("000F653B", "meditation"),
    ("000FC452", "bluetooth_scale_assistant"),
    ("000FFD39", "hrv"),
    ("000FFD3C", "jet_lag_manager"),
    ("00100446", "worldclock2"),
    ("00100F0A", "takephoto2"),
    ("0010132C", "breathing2"),
    ("0010660A", "zepp_flow"),
    ("00108DA2", "podcast"),
    ("0010953E", "zepp_flow2"),
    ("0010A78D", "intervals_icu"),
];

/// Shortcut entries: the screens reached by swiping left and right.
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("00000001", "hr"),
    ("00000002", "weather"),
    ("00000003", "pai"),
    ("00000004", "music"),
    ("00000005", "sleep"),
    ("00000006", "alipay"),
    ("00000008", "wechat_pay"),
    ("00000009", "cards"),
    ("0000000A", "workout"),
    ("0000000B", "workout_history"),
    ("0000000C", "workout_status"),
    ("0000000E", "one_tap_measuring"),
    ("0000000F", "stress"),
    ("00000011", "female_health"),
    ("00000012", "breathing"),
    ("00000013", "spo2"),
    ("00000015", "thermometer"),
    ("00000016", "alarm"),
    ("00000017", "calendar"),
    ("00000018", "events"),
    ("00000019", "todo"),
    ("0000001A", "worldclock"),
    ("0000001B", "compass"),
    ("0000001C", "barometer"),
    ("0000001E", "voice_memos"),
    ("00000020", "activity"),
    ("00000021", "eventreminder"),
    ("00000023", "mi_ai"),
    ("00000025", "alexa"),
    ("00000027", "workout_shortcuts"),
    ("00000028", "apps_shortcuts"),
    ("00000029", "body_composition"),
    ("0000002A", "readiness"),
    ("0000002B", "zepp_pay"),
    ("0000002D", "bio_charge"),
    ("000FFD39", "hrv"),
    ("00100446", "worldclock2"),
    ("0010132C", "breathing2"),
];

/// Control center entries: the panel reached by swiping down.
pub const CONTROL_CENTER: &[(&str, &str)] = &[
    ("00000000", "flashlight"),
    ("00000001", "brightness"),
    ("00000002", "lockscreen"),
    ("00000003", "dnd"),
    ("00000004", "sleep"),
    ("00000005", "findphone"),
    ("00000006", "volume"),
    ("00000007", "battery"),
    ("00000008", "theater_mode"),
    ("00000009", "screen_always_lit"),
    ("0000000A", "bluetooth"),
    ("0000000B", "wifi"),
    ("0000000D", "calendar"),
    ("00000010", "music"),
    ("00000012", "alarm"),
    ("00000013", "settings"),
    ("00000014", "buzzer_intensity"),
    ("00000015", "barometer"),
    ("00000016", "compass"),
    ("00000017", "countdown"),
    ("00000018", "stopwatch"),
    ("00000019", "eject_water"),
    ("0000001A", "headphone"),
    ("0000001B", "night_display"),
    ("0000001C", "always_on_display"),
    ("0000001D", "text_size"),
    ("0000001E", "flashlight2"),
    ("0000001F", "sos_flashlight"),
    ("00000020", "mute"),
];

/// The name for an id, when the band has a known screen there.
pub fn name_for(table: &[(&'static str, &'static str)], id: &str) -> Option<&'static str> {
    table
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(id))
        .map(|(_, name)| *name)
}

/// The id for a name, so a person can type "music" instead of hex.
pub fn id_for(table: &[(&'static str, &'static str)], name: &str) -> Option<&'static str> {
    table
        .iter()
        .find(|(_, known)| known.eq_ignore_ascii_case(name))
        .map(|(id, _)| *id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn music_has_a_different_id_in_each_list() {
        assert_eq!(id_for(MENU, "music"), Some("0000000B"));
        assert_eq!(id_for(SHORTCUTS, "music"), Some("00000004"));
        assert_eq!(id_for(CONTROL_CENTER, "music"), Some("00000010"));
    }

    #[test]
    fn names_and_ids_round_trip() {
        for table in [MENU, SHORTCUTS, CONTROL_CENTER] {
            for (id, name) in table {
                assert_eq!(name_for(table, id), Some(*name));
                assert_eq!(id_for(table, name), Some(*id));
            }
        }
    }

    #[test]
    fn an_app_id_has_no_name() {
        assert_eq!(name_for(MENU, "58E537B5"), None);
    }
}
