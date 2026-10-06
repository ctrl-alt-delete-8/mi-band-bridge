//! Telling the band what time it is.
//!
//! The band has no clock it can trust on its own. It takes the time from
//! whichever host it is paired with, and drifts between those moments. While
//! this program holds the band the phone cannot reach it, so nothing else will
//! set it: minutes of drift build up and show on the watch face.

use chrono::{DateTime, Datelike, Local, Offset, TimeZone, Timelike};

/// The standard GATT current time characteristic.
///
/// A Bluetooth Low Energy Zepp OS device takes its clock here, not on a chunked
/// endpoint. Gadgetbridge writes it in ZeppOsBtleSupport.setCurrentTime, and
/// the band ignores the chunked time service that the other transport uses.
pub const CHAR_CURRENT_TIME: uuid::Uuid = uuid::uuid!("00002a2b-0000-1000-8000-00805f9b34fb");

/// Build the value written to the current time characteristic.
///
/// Eleven bytes, following Gadgetbridge's HuamiUtils.getCurrentTimeBytes. The
/// layout is nearly the GATT current time format, with departures the band
/// insists on: the day of the week counts from zero, and two extra bytes carry
/// the zone. Getting either wrong is refused as out of range.
pub fn set_time_command<Tz: TimeZone>(now: DateTime<Tz>, in_summer_time: bool) -> Vec<u8> {
    let offset_seconds = now.offset().fix().local_minus_utc();

    let mut out = Vec::with_capacity(11);
    out.extend_from_slice(&(now.year() as u16).to_le_bytes());
    out.push(now.month() as u8);
    out.push(now.day() as u8);
    out.push(now.hour() as u8);
    out.push(now.minute() as u8);
    out.push(now.second() as u8);
    // Sunday is zero here. chrono counts Monday as zero, so shift it.
    out.push(now.weekday().num_days_from_sunday() as u8);
    // The fraction of a second, in 1/256ths.
    let fraction = (now.timestamp_subsec_millis() as f64 / 1000.0 * 256.0) as u8;
    out.push(fraction);
    // The daylight saving marker. Gadgetbridge sends 0x08 while summer time is
    // in effect. Sending zero the year round left the band unmoved.
    out.push(if in_summer_time { 0x08 } else { 0x00 });
    // The offset from UTC in quarter hours, signed.
    out.push((offset_seconds / (60 * 15)) as i8 as u8);
    out
}

/// Whether this zone is in summer time right now.
///
/// chrono does not say so directly, so compare today's offset with the one in
/// January. A zone that is further ahead now than in midwinter is on summer
/// time. Southern zones keep working, because their January is the shifted one
/// and the comparison simply runs the other way.
fn in_summer_time(now: DateTime<Local>) -> bool {
    let midwinter = Local
        .with_ymd_and_hms(now.year(), 1, 15, 12, 0, 0)
        .single()
        .map(|winter| winter.offset().fix().local_minus_utc());
    let midsummer = Local
        .with_ymd_and_hms(now.year(), 7, 15, 12, 0, 0)
        .single()
        .map(|summer| summer.offset().fix().local_minus_utc());

    let here = now.offset().fix().local_minus_utc();
    match (midwinter, midsummer) {
        // A zone with no shift at all is never on summer time.
        (Some(winter), Some(summer)) if winter != summer => here == winter.max(summer),
        _ => false,
    }
}

/// The band's clock, set from this machine's clock.
pub fn set_now() -> Vec<u8> {
    let now = Local::now();
    set_time_command(now, in_summer_time(now))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    /// A fixed moment, so the layout is pinned rather than described.
    #[test]
    fn the_layout_is_what_the_band_expects() {
        // 2026-09-06 14:12:30.500 at UTC-4, a Sunday.
        let zone = FixedOffset::east_opt(-4 * 3600).expect("a real offset");
        let now = zone
            .with_ymd_and_hms(2026, 9, 6, 14, 12, 30)
            .single()
            .expect("a real moment");

        let out = set_time_command(now, true);
        assert_eq!(&out[0..2], &2026u16.to_le_bytes());
        assert_eq!(out[2], 9, "month counts from one");
        assert_eq!(out[3], 6);
        assert_eq!(out[4], 14);
        assert_eq!(out[5], 12);
        assert_eq!(out[6], 30);
        assert_eq!(out[7], 0, "Sunday is zero");
        assert_eq!(out[9], 0x08, "summer time is marked");
        assert_eq!(out[10] as i8, -16, "UTC-4 is sixteen quarter hours west");
        assert_eq!(out.len(), 11);
    }

    /// A day of the week counted from Monday would be off by one all week, and
    /// the band would show the wrong day rather than refuse anything.
    #[test]
    fn the_week_starts_on_sunday() {
        let zone = FixedOffset::east_opt(0).expect("a real offset");
        for (day, expected) in [(6, 0u8), (7, 1), (8, 2), (12, 6)] {
            let now = zone
                .with_ymd_and_hms(2026, 9, day, 0, 0, 0)
                .single()
                .expect("a real moment");
            assert_eq!(set_time_command(now, false)[7], expected, "day {day}");
        }
    }

    /// East of UTC the offset is positive, and it must survive the byte.
    #[test]
    fn an_eastern_offset_stays_positive() {
        let zone = FixedOffset::east_opt(9 * 3600 + 30 * 60).expect("a real offset");
        let now = zone
            .with_ymd_and_hms(2026, 9, 6, 0, 0, 0)
            .single()
            .expect("a real moment");
        assert_eq!(set_time_command(now, false)[10] as i8, 38, "UTC+9:30");
    }
}
