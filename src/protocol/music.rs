/// Endpoint used by the stock Zepp OS Music screen.
pub const ENDPOINT_MUSIC: u16 = 0x001b;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicEvent {
    AppOpen,
    AppClose,
    Play,
    Pause,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
}

impl MusicEvent {
    pub fn label(&self) -> &'static str {
        match self {
            MusicEvent::AppOpen => "app_open",
            MusicEvent::AppClose => "app_close",
            MusicEvent::Play => "play",
            MusicEvent::Pause => "pause",
            MusicEvent::Next => "next",
            MusicEvent::Previous => "previous",
            MusicEvent::VolumeUp => "volume_up",
            MusicEvent::VolumeDown => "volume_down",
        }
    }
}

pub fn parse(payload: &[u8]) -> Option<MusicEvent> {
    match payload {
        [0x04, 0x01, ..] => Some(MusicEvent::AppOpen),
        [0x04, 0x02, ..] => Some(MusicEvent::AppClose),
        [0x05, 0x00, ..] => Some(MusicEvent::Play),
        [0x05, 0x01, ..] => Some(MusicEvent::Pause),
        [0x05, 0x03, ..] => Some(MusicEvent::Next),
        [0x05, 0x04, ..] => Some(MusicEvent::Previous),
        [0x05, 0x05, ..] => Some(MusicEvent::VolumeUp),
        [0x05, 0x06, ..] => Some(MusicEvent::VolumeDown),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_every_known_code() {
        let cases: [(&[u8], MusicEvent); 8] = [
            (&[0x04, 0x01], MusicEvent::AppOpen),
            (&[0x04, 0x02], MusicEvent::AppClose),
            (&[0x05, 0x00], MusicEvent::Play),
            (&[0x05, 0x01], MusicEvent::Pause),
            (&[0x05, 0x03], MusicEvent::Next),
            (&[0x05, 0x04], MusicEvent::Previous),
            (&[0x05, 0x05], MusicEvent::VolumeUp),
            (&[0x05, 0x06], MusicEvent::VolumeDown),
        ];
        for (bytes, expected) in cases {
            assert_eq!(parse(bytes), Some(expected), "failed on {bytes:02x?}");
        }
    }

    #[test]
    fn ignores_trailing_bytes() {
        assert_eq!(parse(&[0x05, 0x05, 0xff, 0xff]), Some(MusicEvent::VolumeUp));
    }

    #[test]
    fn returns_none_for_unknown_and_short_payloads() {
        assert_eq!(parse(&[0x05, 0x02]), None);
        assert_eq!(parse(&[0x03, 0x01]), None);
        assert_eq!(parse(&[0x05]), None);
        assert_eq!(parse(&[]), None);
    }

    #[test]
    fn labels_are_snake_case() {
        assert_eq!(MusicEvent::VolumeUp.label(), "volume_up");
        assert_eq!(MusicEvent::AppOpen.label(), "app_open");
    }
}
