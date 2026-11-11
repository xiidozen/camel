//! Java dates: [`Date`] (Java `java.util.Date`, milliseconds since the epoch), [`TimeZone`]
//! (Java `java.util.TimeZone`, with the same ids: IANA names such as `Europe/Paris`, `UTC`,
//! `GMT`, custom `GMT+08:00`) and [`SimpleDateFormat`] (Java `java.text.SimpleDateFormat`
//! patterns such as `yyyy-MM-dd'T'HH:mm:ss.SSSZ`, US English names).
//!
//! The time zone rules come from the IANA time zone database bundled in the binary, so the
//! zones work the same on every platform. The calendar is the proleptic Gregorian calendar
//! (Java switches to the Julian calendar before 1582).
//!
//! # Examples
//!
//! ```
//! use camel_util::date::{Date, SimpleDateFormat, TimeZone};
//!
//! let date = Date::new(135_168_947_123); // 1974-04-14T10:55:47.123Z
//! let df = SimpleDateFormat::new("yyyy-MM-dd'T'HH:mm:ss.SSS").unwrap()
//!     .with_time_zone(TimeZone::get("GMT+8"));
//! assert_eq!("1974-04-14T18:55:47.123", df.format(&date));
//! ```

use crate::value::{ObjectRef, Value};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// A point in time with millisecond precision (Java `java.util.Date`).
///
/// Its text (Java `Date.toString()`) is `EEE MMM dd HH:mm:ss zzz yyyy` in the default time
/// zone, such as `Thu Jan 01 00:00:00 UTC 1970`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    millis: i64,
}

impl Date {
    /// The date of the milliseconds since the epoch (1970-01-01T00:00:00Z).
    pub fn new(millis: i64) -> Self {
        Date { millis }
    }

    /// The current time.
    pub fn now() -> Self {
        let millis = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_millis() as i64,
            Err(e) => -(e.duration().as_millis() as i64),
        };
        Date { millis }
    }

    /// The date of the calendar fields in the time zone (Java `Calendar.set(..)` then
    /// `getTime()`); `month` is 1 to 12. A local time in a gap or overlap of the zone (a
    /// daylight saving change) uses the offset before the change, as Java.
    #[allow(clippy::too_many_arguments)]
    pub fn of(
        zone: &TimeZone,
        year: i64,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
        millis: u32,
    ) -> Date {
        let local = days_from_civil(year, month, day) * 86_400_000
            + hour as i64 * 3_600_000
            + minute as i64 * 60_000
            + second as i64 * 1000
            + millis as i64;
        // the offset at the local time read as UTC, then at the resulting instant
        let first = zone.offset_at(&Date::new(local)).0 as i64 * 1000;
        let second_offset = zone.offset_at(&Date::new(local - first)).0 as i64 * 1000;
        Date::new(local - second_offset)
    }

    /// The milliseconds since the epoch (Java `getTime()`).
    pub fn millis(&self) -> i64 {
        self.millis
    }

    /// The date as a [`Value::Object`] (equal to another value holding the same date).
    pub fn to_value(self) -> Value {
        Value::Object(ObjectRef::new_display(self).with_equals::<Date>())
    }

    /// The date of a value: a [`Date`] object, or an integer number of milliseconds (Java
    /// `DateTimeConverter`).
    pub fn from_value(value: &Value) -> Option<Date> {
        match value {
            Value::Int(i) => Some(Date::new(*i)),
            Value::Object(o) => o.downcast_ref::<Date>().copied(),
            _ => None,
        }
    }
}

impl fmt::Display for Date {
    /// Java `Date.toString()`: `EEE MMM dd HH:mm:ss zzz yyyy` in the default time zone.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let df = SimpleDateFormat::new("EEE MMM dd HH:mm:ss zzz yyyy")
            .expect("valid pattern")
            .with_time_zone(TimeZone::default_zone());
        f.write_str(&df.format(self))
    }
}

impl From<Date> for Value {
    fn from(d: Date) -> Self {
        d.to_value()
    }
}

/// A time zone (Java `java.util.TimeZone`).
#[derive(Debug, Clone)]
pub struct TimeZone {
    id: String,
    kind: ZoneKind,
}

#[derive(Debug, Clone)]
enum ZoneKind {
    /// A fixed offset in seconds (`GMT`, `UTC`, `GMT+08:00`).
    Fixed(i32),
    /// The rules of the IANA database.
    Rules(jiff::tz::TimeZone),
}

impl TimeZone {
    /// The time zone of the id (Java `TimeZone.getTimeZone(id)`): an IANA name such as
    /// `Europe/Paris`, `UTC`, `GMT`, or a custom id `GMT+8`, `GMT-0530`, `GMT+02:00`. An
    /// unknown id is `GMT`, as in Java.
    pub fn get(id: &str) -> TimeZone {
        if id == "GMT" || id == "UTC" {
            return TimeZone {
                id: id.to_string(),
                kind: ZoneKind::Fixed(0),
            };
        }
        if let Some(rest) = id.strip_prefix("GMT") {
            return match parse_custom_offset(rest) {
                Some(secs) => TimeZone {
                    id: custom_id(secs),
                    kind: ZoneKind::Fixed(secs),
                },
                None => TimeZone::get("GMT"),
            };
        }
        match jiff::tz::TimeZone::get(id) {
            Ok(tz) => TimeZone {
                id: id.to_string(),
                kind: ZoneKind::Rules(tz),
            },
            Err(_) => TimeZone::get("GMT"),
        }
    }

    /// The default time zone (Java `TimeZone.getDefault()`): the `user.timezone` system
    /// property, else the `TZ` environment variable, else the time zone of the system, else
    /// `UTC`.
    pub fn default_zone() -> TimeZone {
        if let Some(id) = crate::system_properties::get_property("user.timezone") {
            if !id.is_empty() {
                return TimeZone::get(&id);
            }
        }
        if let Ok(id) = std::env::var("TZ") {
            let id = id.trim_start_matches(':');
            if !id.is_empty() {
                return TimeZone::get(id);
            }
        }
        match jiff::tz::TimeZone::try_system() {
            Ok(tz) => TimeZone {
                id: tz.iana_name().unwrap_or("UTC").to_string(),
                kind: ZoneKind::Rules(tz),
            },
            Err(_) => TimeZone::get("UTC"),
        }
    }

    /// The id (Java `getID()`); a custom id is normalized to `GMT+hh:mm`.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The offset from UTC in seconds at the date, and the short name of the zone then (such
    /// as `CET` or `CEST`).
    pub fn offset_at(&self, date: &Date) -> (i32, String) {
        match &self.kind {
            ZoneKind::Fixed(secs) => (*secs, self.id.clone()),
            ZoneKind::Rules(tz) => {
                let ts = jiff::Timestamp::from_millisecond(date.millis)
                    .unwrap_or(jiff::Timestamp::UNIX_EPOCH);
                let info = tz.to_offset_info(ts);
                let secs = info.offset().seconds();
                let abbreviation = info.abbreviation();
                // Java names an offset without a name GMT+hh:mm
                let name = if abbreviation.starts_with(['+', '-']) || abbreviation.is_empty() {
                    custom_id(secs)
                } else {
                    abbreviation.to_string()
                };
                (secs, name)
            }
        }
    }
}

/// `+8`, `-0530`, `+02:00` as seconds.
fn parse_custom_offset(text: &str) -> Option<i32> {
    let (sign, rest) = match text.chars().next()? {
        '+' => (1, &text[1..]),
        '-' => (-1, &text[1..]),
        _ => return None,
    };
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    let (h, m) = if let Some((h, m)) = rest.split_once(':') {
        if !digits(h) || h.len() > 2 || m.len() != 2 || !digits(m) {
            return None;
        }
        (h.parse::<i32>().ok()?, m.parse::<i32>().ok()?)
    } else if digits(rest) && rest.len() <= 2 {
        (rest.parse::<i32>().ok()?, 0)
    } else if digits(rest) && (rest.len() == 3 || rest.len() == 4) {
        let split = rest.len() - 2;
        (rest[..split].parse().ok()?, rest[split..].parse().ok()?)
    } else {
        return None;
    };
    if h > 23 || m > 59 {
        return None;
    }
    Some(sign * (h * 3600 + m * 60))
}

/// `GMT+hh:mm` of the offset (Java custom time zone ids).
fn custom_id(secs: i32) -> String {
    if secs == 0 {
        return "GMT".to_string();
    }
    let sign = if secs < 0 { '-' } else { '+' };
    let abs = secs.abs();
    format!("GMT{sign}{:02}:{:02}", abs / 3600, (abs % 3600) / 60)
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// The calendar fields of a date in a time zone.
#[derive(Debug, Clone, Copy)]
struct Fields {
    year: i64,
    /// 1..=12
    month: u32,
    /// 1..=31
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millis: u32,
    /// 0 = Sunday
    weekday: u32,
    /// 1..=366
    day_of_year: u32,
}

/// The civil date of the days since the epoch (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The days since the epoch of the civil date.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn fields(local_millis: i64) -> Fields {
    let days = local_millis.div_euclid(86_400_000);
    let ms = local_millis.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    Fields {
        year,
        month,
        day,
        hour: (ms / 3_600_000) as u32,
        minute: ((ms / 60_000) % 60) as u32,
        second: ((ms / 1000) % 60) as u32,
        millis: (ms % 1000) as u32,
        // 1970-01-01 was a Thursday
        weekday: (days + 4).rem_euclid(7) as u32,
        day_of_year: (days - days_from_civil(year, 1, 1) + 1) as u32,
    }
}

/// The week of the year and the week year (Java US locale: weeks start on Sunday, the week
/// with January 1st is week 1).
fn week_of_year(f: &Fields) -> (u32, i64) {
    // the weekday of January 1st
    let jan1 = (f.weekday as i64 - (f.day_of_year as i64 - 1)).rem_euclid(7);
    let week = ((f.day_of_year as i64 - 1 + jan1) / 7 + 1) as u32;
    // the last days of December may be in week 1 of the next year
    let days_in_year = days_from_civil(f.year + 1, 1, 1) - days_from_civil(f.year, 1, 1);
    let next_jan1 = (jan1 + days_in_year).rem_euclid(7);
    if next_jan1 != 0 && f.day_of_year as i64 > days_in_year - next_jan1 {
        return (1, f.year + 1);
    }
    (week, f.year)
}

/// The week of the month (Java US locale).
fn week_of_month(f: &Fields) -> u32 {
    let first = (f.weekday as i64 - (f.day as i64 - 1)).rem_euclid(7);
    ((f.day as i64 - 1 + first) / 7 + 1) as u32
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Literal(String),
    Field(char, usize),
}

/// The Java pattern letters of `SimpleDateFormat`.
const PATTERN_LETTERS: &str = "GyMdkHmsSEDFwWahKzZYuXL";

/// A date format with a Java `SimpleDateFormat` pattern (US English names).
///
/// The letters: `G` era, `y` year, `Y` week year, `M`/`L` month (`MMM` short name, `MMMM`
/// full name), `w` week in year, `W` week in month, `D` day in year, `d` day in month, `F`
/// day of week in month, `E` day name (`EEEE` full), `u` day number (1 = Monday), `a` AM/PM,
/// `H` hour 0-23, `k` hour 1-24, `K` hour 0-11, `h` hour 1-12, `m` minute, `s` second, `S`
/// millisecond, `z` time zone name, `Z` RFC 822 offset (`-0800`), `X` ISO 8601 offset (`-08`,
/// `-0800`, `-08:00`). Text in single quotes is literal, `''` is a quote.
#[derive(Debug, Clone)]
pub struct SimpleDateFormat {
    pattern: String,
    parts: Vec<Part>,
    zone: Option<TimeZone>,
}

impl SimpleDateFormat {
    /// The format of the pattern, in the default time zone.
    ///
    /// # Errors
    ///
    /// Returns the Java `IllegalArgumentException` message for an invalid pattern, such as
    /// `Illegal pattern character 'b'` or `Unterminated quote`.
    pub fn new(pattern: &str) -> Result<SimpleDateFormat, String> {
        let chars: Vec<char> = pattern.chars().collect();
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '\'' {
                if chars.get(i + 1) == Some(&'\'') {
                    literal.push('\'');
                    i += 2;
                    continue;
                }
                // quoted text up to the closing quote, '' inside is a quote
                i += 1;
                let mut closed = false;
                while i < chars.len() {
                    if chars[i] == '\'' {
                        if chars.get(i + 1) == Some(&'\'') {
                            literal.push('\'');
                            i += 2;
                            continue;
                        }
                        closed = true;
                        i += 1;
                        break;
                    }
                    literal.push(chars[i]);
                    i += 1;
                }
                if !closed {
                    return Err("Unterminated quote".to_string());
                }
                continue;
            }
            if c.is_ascii_alphabetic() {
                if !PATTERN_LETTERS.contains(c) {
                    return Err(format!("Illegal pattern character '{c}'"));
                }
                let mut count = 1;
                while chars.get(i + count) == Some(&c) {
                    count += 1;
                }
                if c == 'X' && count > 3 {
                    return Err(format!("invalid ISO 8601 format: length={count}"));
                }
                if !literal.is_empty() {
                    parts.push(Part::Literal(std::mem::take(&mut literal)));
                }
                parts.push(Part::Field(c, count));
                i += count;
                continue;
            }
            literal.push(c);
            i += 1;
        }
        if !literal.is_empty() {
            parts.push(Part::Literal(literal));
        }
        Ok(SimpleDateFormat {
            pattern: pattern.to_string(),
            parts,
            zone: None,
        })
    }

    /// Uses the time zone (Java `setTimeZone`).
    pub fn with_time_zone(mut self, zone: TimeZone) -> Self {
        self.zone = Some(zone);
        self
    }

    /// The pattern (Java `toPattern()`).
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Formats the date (Java `format(Date)`).
    pub fn format(&self, date: &Date) -> String {
        let zone = match &self.zone {
            Some(z) => z.clone(),
            None => TimeZone::default_zone(),
        };
        let (offset, zone_name) = zone.offset_at(date);
        let f = fields(date.millis + offset as i64 * 1000);
        let mut out = String::new();
        let pad = |out: &mut String, n: i64, width: usize| {
            if n < 0 {
                out.push('-');
            }
            out.push_str(&format!("{:0width$}", n.unsigned_abs(), width = width));
        };
        for part in &self.parts {
            match part {
                Part::Literal(s) => out.push_str(s),
                Part::Field(c, count) => {
                    let n = *count;
                    match c {
                        'G' => out.push_str(if f.year > 0 { "AD" } else { "BC" }),
                        'y' | 'Y' => {
                            let year = if *c == 'Y' {
                                week_of_year(&f).1
                            } else {
                                f.year
                            };
                            // the year of the era
                            let year = if year > 0 { year } else { 1 - year };
                            if n == 2 {
                                pad(&mut out, year % 100, 2);
                            } else {
                                pad(&mut out, year, n);
                            }
                        }
                        'M' | 'L' => {
                            if n >= 4 {
                                out.push_str(MONTHS[f.month as usize - 1]);
                            } else if n == 3 {
                                out.push_str(&MONTHS[f.month as usize - 1][..3]);
                            } else {
                                pad(&mut out, f.month as i64, n);
                            }
                        }
                        'd' => pad(&mut out, f.day as i64, n),
                        'D' => pad(&mut out, f.day_of_year as i64, n),
                        'F' => pad(&mut out, ((f.day - 1) / 7 + 1) as i64, n),
                        'w' => pad(&mut out, week_of_year(&f).0 as i64, n),
                        'W' => pad(&mut out, week_of_month(&f) as i64, n),
                        'E' => {
                            let day = DAYS[f.weekday as usize];
                            out.push_str(if n >= 4 { day } else { &day[..3] });
                        }
                        'u' => {
                            let u = if f.weekday == 0 { 7 } else { f.weekday };
                            pad(&mut out, u as i64, n);
                        }
                        'a' => out.push_str(if f.hour < 12 { "AM" } else { "PM" }),
                        'H' => pad(&mut out, f.hour as i64, n),
                        'k' => pad(&mut out, if f.hour == 0 { 24 } else { f.hour } as i64, n),
                        'K' => pad(&mut out, (f.hour % 12) as i64, n),
                        'h' => {
                            let h = f.hour % 12;
                            pad(&mut out, if h == 0 { 12 } else { h } as i64, n);
                        }
                        'm' => pad(&mut out, f.minute as i64, n),
                        's' => pad(&mut out, f.second as i64, n),
                        'S' => pad(&mut out, f.millis as i64, n),
                        'z' => out.push_str(&zone_name),
                        'Z' => out.push_str(&offset_text(offset, false, false)),
                        'X' => {
                            if offset == 0 {
                                out.push('Z');
                            } else if n == 1 {
                                let s = offset_text(offset, false, false);
                                out.push_str(&s[..3]);
                            } else {
                                out.push_str(&offset_text(offset, n == 3, false));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        out
    }
}

/// `+0800` (or `+08:00` with the colon) of the offset in seconds.
fn offset_text(secs: i32, colon: bool, _gmt: bool) -> String {
    let sign = if secs < 0 { '-' } else { '+' };
    let abs = secs.abs();
    let (h, m) = (abs / 3600, (abs % 3600) / 60);
    if colon {
        format!("{sign}{h:02}:{m:02}")
    } else {
        format!("{sign}{h:02}{m:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(pattern: &str, millis: i64) -> String {
        SimpleDateFormat::new(pattern)
            .unwrap()
            .with_time_zone(TimeZone::get("UTC"))
            .format(&Date::new(millis))
    }

    #[test]
    fn test_civil_round_trip() {
        for days in [-719_468, -1, 0, 1, 10_957, 19_000, 2_932_896] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days, days_from_civil(y, m, d));
        }
        assert_eq!((1970, 1, 1), civil_from_days(0));
        assert_eq!((2000, 2, 29), civil_from_days(11_016));
    }

    #[test]
    fn test_format_fields() {
        // 1974-04-20T08:55:47.123Z, a Saturday
        let millis = 135_680_147_123;
        assert_eq!(
            "1974-04-20T08:55:47:123",
            utc("yyyy-MM-dd'T'HH:mm:ss:SSS", millis)
        );
        assert_eq!("19740420", utc("yyyyMMdd", millis));
        assert_eq!("74 Apr April 4", utc("yy MMM MMMM M", millis));
        assert_eq!("Sat Saturday 6", utc("EEE EEEE u", millis));
        assert_eq!("08:55:47 AM", utc("hh:mm:ss a", millis));
        assert_eq!("110 3 16 3", utc("D F w W", millis));
        assert_eq!("AD +0000 Z UTC", utc("G Z X z", millis));
        assert_eq!("it's 8", utc("'it''s' H", millis));
        assert_eq!("12 24 0", utc("h k K", 0));
    }

    #[test]
    fn test_invalid_patterns() {
        assert_eq!(
            "Illegal pattern character 'b'",
            SimpleDateFormat::new("yyyy-bb").unwrap_err()
        );
        assert_eq!(
            "Unterminated quote",
            SimpleDateFormat::new("yyyy 'T").unwrap_err()
        );
        assert_eq!(
            "invalid ISO 8601 format: length=4",
            SimpleDateFormat::new("XXXX").unwrap_err()
        );
    }

    #[test]
    fn test_time_zones() {
        let date = Date::new(0);
        let format = |zone: &str, pattern: &str| {
            SimpleDateFormat::new(pattern)
                .unwrap()
                .with_time_zone(TimeZone::get(zone))
                .format(&date)
        };
        assert_eq!("02:00", format("GMT+02:00", "HH:mm"));
        assert_eq!("08:00 GMT+08:00", format("GMT+8", "HH:mm z"));
        assert_eq!(
            "19:00 -0500 -05:00 -05",
            format("GMT-0500", "HH:mm Z XXX X")
        );
        assert_eq!("00:00 GMT", format("Unknown/Zone", "HH:mm z"));
        assert_eq!("01:00 CET +0100", format("Europe/Paris", "HH:mm z Z"));
        let summer = Date::new(14_400_000_000); // 1970-06-16T16:00Z
        let df = SimpleDateFormat::new("HH z")
            .unwrap()
            .with_time_zone(TimeZone::get("America/Denver"));
        assert_eq!("10 MDT", df.format(&summer));
        assert_eq!("GMT+08:00", TimeZone::get("GMT+8").id());
    }

    #[test]
    fn test_of_calendar_fields() {
        let paris = TimeZone::get("Europe/Paris");
        let d = Date::of(&paris, 1974, 4, 20, 8, 55, 47, 123);
        let df = SimpleDateFormat::new("yyyy-MM-dd HH:mm:ss.SSS z")
            .unwrap()
            .with_time_zone(paris.clone());
        assert_eq!("1974-04-20 08:55:47.123 CET", df.format(&d));
        let summer = Date::of(&paris, 2026, 7, 1, 12, 0, 0, 0);
        assert_eq!("2026-07-01 12:00:00.000 CEST", df.format(&summer));
        assert_eq!(
            0,
            Date::of(&TimeZone::get("UTC"), 1970, 1, 1, 0, 0, 0, 0).millis()
        );
    }

    #[test]
    fn test_week_year() {
        // 2026-12-31 is a Thursday: week 1 of 2027 in the US calendar
        let millis = days_from_civil(2026, 12, 31) * 86_400_000;
        assert_eq!("1 2027 2026", utc("w YYYY yyyy", millis));
    }

    #[test]
    fn test_date_value() {
        let v = Date::new(1000).to_value();
        assert_eq!(v, Date::new(1000).to_value());
        assert_ne!(v, Date::new(2000).to_value());
        assert_eq!(Some(Date::new(1000)), Date::from_value(&v));
        assert_eq!(Some(Date::new(5)), Date::from_value(&Value::Int(5)));
    }
}
