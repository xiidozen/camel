//! Time utils. Port of `org.apache.camel.util.TimeUtils`.

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A signed duration with nanosecond precision, the equivalent of `java.time.Duration`.
///
/// Rust's [`std::time::Duration`] cannot be negative, whereas Camel accepts negative
/// durations such as `-PT30S`. The value is stored as whole seconds plus a nanosecond
/// adjustment in `0..1_000_000_000`, exactly like the Java type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct SignedDuration {
    seconds: i64,
    nanos: u32,
}

impl SignedDuration {
    /// A duration of zero length.
    pub const ZERO: SignedDuration = SignedDuration {
        seconds: 0,
        nanos: 0,
    };

    /// Obtains a duration from a number of seconds and a nanosecond adjustment (which may be
    /// negative or larger than a second; it is normalized).
    pub fn of_seconds_nanos(seconds: i64, nano_adjustment: i64) -> Self {
        let secs = seconds + nano_adjustment.div_euclid(1_000_000_000);
        let nanos = nano_adjustment.rem_euclid(1_000_000_000) as u32;
        SignedDuration {
            seconds: secs,
            nanos,
        }
    }

    /// Obtains a duration representing a number of seconds.
    pub fn of_seconds(seconds: i64) -> Self {
        SignedDuration { seconds, nanos: 0 }
    }

    /// Obtains a duration representing a number of milliseconds.
    pub fn of_millis(millis: i64) -> Self {
        Self::of_seconds_nanos(millis.div_euclid(1000), millis.rem_euclid(1000) * 1_000_000)
    }

    /// Obtains a duration representing a number of nanoseconds.
    pub fn of_nanos(nanos: i64) -> Self {
        Self::of_seconds_nanos(0, nanos)
    }

    /// The number of whole seconds (may be negative).
    pub fn seconds(&self) -> i64 {
        self.seconds
    }

    /// The nanosecond adjustment within the second, in `0..1_000_000_000`.
    pub fn nano(&self) -> u32 {
        self.nanos
    }

    /// Whether this duration is negative (excluding zero).
    pub fn is_negative(&self) -> bool {
        self.seconds < 0
    }

    /// Whether this duration is zero length.
    pub fn is_zero(&self) -> bool {
        self.seconds == 0 && self.nanos == 0
    }

    /// Converts this duration to the total length in milliseconds (truncating, like Java).
    pub fn to_millis(&self) -> i64 {
        let millis = self.seconds.saturating_mul(1000);
        // Java adds nanos / 1_000_000 (nanos is always positive)
        millis.saturating_add((self.nanos / 1_000_000) as i64)
    }

    /// Converts to a [`std::time::Duration`], or `None` if this duration is negative.
    pub fn to_std(&self) -> Option<Duration> {
        if self.is_negative() {
            None
        } else {
            Some(Duration::new(self.seconds as u64, self.nanos))
        }
    }

    /// Obtains a duration from a text string such as `PnDTnHnMn.nS` (ISO-8601, Java
    /// `Duration.parse`).
    ///
    /// The letters are case-insensitive, each number may be signed, and the whole text may be
    /// prefixed with `-` or `+`.
    ///
    /// # Errors
    ///
    /// Returns an error if the text cannot be parsed to a duration.
    ///
    /// # Examples
    ///
    /// ```
    /// use camel_util::time_utils::SignedDuration;
    /// assert_eq!(SignedDuration::of_seconds(20), SignedDuration::parse("PT20S").unwrap());
    /// assert_eq!(SignedDuration::of_seconds(-30), SignedDuration::parse("-PT30S").unwrap());
    /// assert_eq!(SignedDuration::of_millis(90_500), SignedDuration::parse("pt1m30.5s").unwrap());
    /// assert_eq!(SignedDuration::of_seconds(2 * 86400 + 3600), SignedDuration::parse("P2DT1H").unwrap());
    /// ```
    pub fn parse(text: &str) -> Result<SignedDuration, String> {
        use regex::Regex;
        use std::sync::OnceLock;
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| {
            Regex::new(
                r"(?i)^([-+]?)P(?:([-+]?[0-9]+)D)?(T(?:([-+]?[0-9]+)H)?(?:([-+]?[0-9]+)M)?(?:([-+]?[0-9]+)(?:[.,]([0-9]{0,9}))?S)?)?$",
            )
            .unwrap()
        });
        let err = || format!("Text cannot be parsed to a Duration: {text}");
        let caps = re.captures(text).ok_or_else(err)?;
        let t_section = caps.get(3).map(|m| m.as_str()).unwrap_or("");
        if t_section.eq_ignore_ascii_case("T") {
            return Err(err());
        }
        if caps.get(2).is_none() && caps.get(3).is_none() {
            return Err(err());
        }
        let num = |i: usize| -> Result<i64, String> {
            match caps.get(i) {
                None => Ok(0),
                Some(m) => m.as_str().parse::<i64>().map_err(|_| err()),
            }
        };
        let days = num(2)?;
        let hours = num(4)?;
        let minutes = num(5)?;
        let seconds = num(6)?;
        let negative_secs = caps
            .get(6)
            .map(|m| m.as_str().starts_with('-'))
            .unwrap_or(false);
        let fraction = match caps.get(7) {
            Some(m) if !m.as_str().is_empty() => {
                let f = format!("{:0<9}", m.as_str());
                let v: i64 = f.parse().map_err(|_| err())?;
                if negative_secs {
                    -v
                } else {
                    v
                }
            }
            _ => 0,
        };
        let total = days
            .checked_mul(86400)
            .and_then(|d| hours.checked_mul(3600).and_then(|h| d.checked_add(h)))
            .and_then(|t| minutes.checked_mul(60).and_then(|m| t.checked_add(m)))
            .and_then(|t| t.checked_add(seconds))
            .ok_or_else(err)?;
        let d = SignedDuration::of_seconds_nanos(total, fraction);
        if caps.get(1).map(|m| m.as_str() == "-").unwrap_or(false) {
            Ok(d.negated())
        } else {
            Ok(d)
        }
    }

    /// Returns a copy of this duration with the length negated.
    pub fn negated(&self) -> SignedDuration {
        SignedDuration::of_seconds_nanos(-self.seconds, -(self.nanos as i64))
    }
}

impl From<Duration> for SignedDuration {
    fn from(d: Duration) -> Self {
        SignedDuration::of_seconds_nanos(d.as_secs() as i64, d.subsec_nanos() as i64)
    }
}

impl fmt::Display for SignedDuration {
    /// Formats as ISO-8601 such as `PT20S` or `PT-0.5S` (the same algorithm as Java
    /// `Duration.toString()`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_zero() {
            return f.write_str("PT0S");
        }
        let nanos = self.nanos as i64;
        let mut effective_total_secs = self.seconds;
        if self.seconds < 0 && nanos > 0 {
            effective_total_secs += 1;
        }
        let hours = effective_total_secs / 3600;
        let minutes = (effective_total_secs % 3600) / 60;
        let secs = effective_total_secs % 60;
        let mut buf = String::from("PT");
        if hours != 0 {
            buf.push_str(&format!("{hours}H"));
        }
        if minutes != 0 {
            buf.push_str(&format!("{minutes}M"));
        }
        if secs == 0 && nanos == 0 && buf.len() > 2 {
            return f.write_str(&buf);
        }
        if self.seconds < 0 && nanos > 0 && secs == 0 {
            buf.push_str("-0");
        } else {
            buf.push_str(&secs.to_string());
        }
        if nanos > 0 {
            let frac = if self.seconds < 0 {
                2 * 1_000_000_000 - nanos
            } else {
                nanos + 1_000_000_000
            };
            // drop the leading digit (it only pads the fraction) and the trailing zeros
            let digits = frac.to_string();
            buf.push('.');
            buf.push_str(digits[1..].trim_end_matches('0'));
        }
        buf.push('S');
        f.write_str(&buf)
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Whether the duration is positive (greater than zero).
pub fn is_positive(dur: &SignedDuration) -> bool {
    dur.seconds() > 0 || (dur.seconds() == 0 && dur.nano() != 0)
}

/// Prints since age in a human-readable format as 9s, 27m44s, 3h12m, 3d8h, as seen on
/// Kubernetes etc.
///
/// # Arguments
///
/// * `time` - time of the event (millis since epoch)
///
/// # Returns
///
/// Age in human-readable since the given time.
pub fn print_since(time: i64) -> String {
    print_duration_precise(now_millis() - time, false)
}

/// Prints since age in a human-readable format as 9s, 27m44s, 3h12m, 3d8h, as seen on
/// Kubernetes etc.
///
/// # Arguments
///
/// * `time` - time of the event (millis since epoch)
/// * `precise` - whether to be precise and include more details
///
/// # Returns
///
/// Age in human-readable since the given time.
pub fn print_since_precise(time: i64, precise: bool) -> String {
    print_duration_precise(now_millis() - time, precise)
}

/// Prints the age in a human-readable format as 9s, 27m44s, 3h12m, 3d8h, as seen on
/// Kubernetes etc.
///
/// # Arguments
///
/// * `age` - age in millis
///
/// # Returns
///
/// Age in human-readable.
pub fn print_age(age: i64) -> String {
    print_duration_precise(age, false)
}

/// Prints the age in a human-readable format as 9s, 27m44s, 3h12m, 3d8h, as seen on
/// Kubernetes etc.
///
/// # Arguments
///
/// * `age` - age in millis
/// * `precise` - whether to be precise and include more details
///
/// # Returns
///
/// Age in human-readable.
pub fn print_age_precise(age: i64, precise: bool) -> String {
    print_duration_precise(age, precise)
}

/// Prints the duration in a human-readable format as 9s, 27m44s, 3h12m, 3d8h, etc.
///
/// # Arguments
///
/// * `uptime` - the uptime
///
/// # Returns
///
/// The time used for displaying on screen or in logs.
pub fn print_signed_duration(uptime: &SignedDuration, precise: bool) -> String {
    print_duration_precise(uptime.to_millis(), precise)
}

/// Prints the duration in a human-readable format as 9s, 27m44s, 3h12m, 3d8h, etc.
///
/// # Arguments
///
/// * `uptime` - the uptime in millis
///
/// # Returns
///
/// The time used for displaying on screen or in logs.
pub fn print_duration(uptime: i64) -> String {
    print_duration_precise(uptime, false)
}

/// Prints the duration in a human-readable format as 9s, 27m44s, 3h12m, 3d8h, etc.
///
/// # Arguments
///
/// * `uptime` - the uptime in millis
/// * `precise` - whether to be precise and include more details
///
/// # Returns
///
/// The time used for displaying on screen or in logs.
///
/// # Examples
///
/// ```
/// use camel_util::time_utils::print_duration_precise;
/// assert_eq!("30m55s", print_duration_precise(1855123, false));
/// assert_eq!("30m55s123ms", print_duration_precise(1855123, true));
/// assert_eq!("2d23h", print_duration_precise(259032000, false));
/// ```
pub fn print_duration_precise(uptime: i64, precise: bool) -> String {
    if uptime <= 0 {
        return "0ms".to_string();
    }
    let seconds = uptime / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    let millis = if uptime > 1000 {
        uptime % 1000
    } else if uptime < 1000 {
        uptime
    } else {
        0
    };
    let mut sb = String::new();
    if days > 0 {
        sb.push_str(&format!("{}d{}h", days, hours % 24));
        if precise {
            sb.push_str(&format!("{}m{}s", minutes % 60, seconds % 60));
        }
    } else if hours > 0 {
        sb.push_str(&format!("{}h{}m", hours % 24, minutes % 60));
        if precise {
            sb.push_str(&format!("{}s", seconds % 60));
        }
    } else if minutes > 0 {
        sb.push_str(&format!("{}m{}s", minutes % 60, seconds % 60));
        if precise {
            sb.push_str(&format!("{millis}ms"));
        }
    } else if seconds > 0 {
        sb.push_str(&format!("{}s", seconds % 60));
        if precise {
            sb.push_str(&format!("{millis}ms"));
        }
    } else if millis > 0 {
        if !precise {
            // less than a second so just report it as zero
            sb.push_str("0s");
        } else {
            sb.push_str(&format!("{millis}ms"));
        }
    }
    sb
}

/// Converts to duration.
///
/// # Arguments
///
/// * `source` - duration which can be in text format such as `15s`, `1m30s` or ISO-8601 such
///   as `PT20S`
///
/// # Errors
///
/// Returns an error if the input is not a number, a valid time pattern or ISO-8601 duration.
pub fn to_duration(source: &str) -> Result<SignedDuration, String> {
    if source.starts_with('P')
        || source.starts_with("-P")
        || source.starts_with('p')
        || source.starts_with("-p")
    {
        SignedDuration::parse(source)
    } else {
        to_milli_seconds(source).map(SignedDuration::of_millis)
    }
}

/// Converts to milliseconds.
///
/// The text can be a plain number of milliseconds, or groups of a number followed by a unit
/// (`d`, `h`, `m`, `s`, `ms` and their long forms such as `hours` or `millis`), optionally
/// separated by whitespace and prefixed with `-`, such as `1h30m` or `5s`.
///
/// # Arguments
///
/// * `source` - duration which can be in text format such as `15s`
///
/// # Returns
///
/// Time in millis, `0` if the input is empty.
///
/// # Errors
///
/// Returns `Invalid time pattern: <source>` if the input is not a number or a valid time
/// pattern (the Java version threw `IllegalArgumentException`).
///
/// # Examples
///
/// ```
/// use camel_util::time_utils::to_milli_seconds;
/// assert_eq!(Ok(5_400_000), to_milli_seconds("1h30m"));
/// assert_eq!(Ok(-2_000), to_milli_seconds("-2secs"));
/// // whitespace is only allowed between groups, not between a number and its unit
/// assert!(to_milli_seconds("2 secs").is_err());
/// assert_eq!(Ok(123), to_milli_seconds("123"));
/// assert!(to_milli_seconds("12x").is_err());
/// ```
pub fn to_milli_seconds(source: &str) -> Result<i64, String> {
    if source.is_empty() {
        return Ok(0);
    }
    // quick conversion if its only digits (the first char can be a negative sign)
    let digit = source
        .chars()
        .enumerate()
        .all(|(i, ch)| (i == 0 && ch == '-') || ch.is_ascii_digit());
    if digit {
        return source
            .parse::<i64>()
            .map_err(|e| format!("For input string: \"{source}\": {e}"));
    }
    parse_time_pattern(source)
}

fn parse_time_pattern(source: &str) -> Result<i64, String> {
    let invalid = || format!("Invalid time pattern: {source}");
    let mut text = crate::lang::java_trim(source);
    let negative = text.starts_with('-');
    if negative {
        text = &text[1..];
    }
    if text.is_empty() {
        return Err(invalid());
    }
    // the whole text must be groups of a number followed by an unit (such as 1h30m or 5s)
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut answer: i64 = 0;
    let mut i = 0;
    while i < len {
        let start = i;
        while i < len && chars[i].is_ascii_digit() {
            i += 1;
        }
        if start == i {
            return Err(invalid());
        }
        let num: i64 = chars[start..i]
            .iter()
            .collect::<String>()
            .parse()
            .map_err(|_| invalid())?;
        let start = i;
        while i < len && chars[i].is_alphabetic() {
            i += 1;
        }
        let unit = chars[start..i].iter().collect::<String>().to_lowercase();
        let factor: i64 = match unit.as_str() {
            "d" | "day" | "days" => 86_400_000,
            "h" | "hour" | "hours" => 3_600_000,
            "m" | "min" | "mins" | "minute" | "minutes" => 60_000,
            "s" | "sec" | "secs" | "second" | "seconds" => 1_000,
            "ms" | "milli" | "millis" => 1,
            _ => return Err(invalid()),
        };
        answer = num
            .checked_mul(factor)
            .and_then(|v| answer.checked_add(v))
            .ok_or_else(invalid)?;
        // allow whitespace between the groups such as 1h 30m
        while i < len && chars[i].is_whitespace() {
            i += 1;
        }
    }
    Ok(if negative { -answer } else { answer })
}

/// Elapsed time using milliseconds since epoch.
///
/// # Arguments
///
/// * `start` - the timestamp in milliseconds since epoch
///
/// # Returns
///
/// The elapsed time in milliseconds.
#[deprecated(since = "4.4.0", note = "use std::time::Instant::elapsed")]
pub fn elapsed_millis_since(start: i64) -> i64 {
    now_millis() - start
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_duration() {
        let p = print_duration_precise;
        assert_eq!("0s", print_duration(123));
        assert_eq!("123ms", p(123, true));
        assert_eq!("1s", print_duration(1250));
        assert_eq!("1s250ms", p(1250, true));
        assert_eq!("33s", print_duration(33000));
        assert_eq!("33s", print_duration(33001));
        assert_eq!("33s1ms", p(33001, true));
        assert_eq!("33s", print_duration(33444));
        assert_eq!("33s444ms", p(33444, true));
        assert_eq!("1m0s", print_duration(60000));
        assert_eq!("1m1s", print_duration(61000));
        assert_eq!("1m1s", print_duration(61002));
        assert_eq!("1m1s2ms", p(61002, true));
        assert_eq!("30m55s", print_duration(1855123));
        assert_eq!("30m55s123ms", p(1855123, true));
        assert_eq!("1h30m", print_duration(5400000));
        assert_eq!("1h30m1s", p(5401000, true));
        assert_eq!("2d23h", print_duration(259032000));
        assert_eq!("2d23h57m12s", p(259032000, true));
        assert_eq!("0ms", print_duration(0));
    }

    #[test]
    fn test_print_age() {
        let cases = [
            (123, "0s"),
            (1250, "1s"),
            (33000, "33s"),
            (33001, "33s"),
            (33444, "33s"),
            (60000, "1m0s"),
            (61000, "1m1s"),
            (61002, "1m1s"),
            (1855123, "30m55s"),
            (5400000, "1h30m"),
            (5401000, "1h30m"),
            (259032000, "2d23h"),
        ];
        for (age, expected) in cases {
            assert_eq!(expected, print_age(age));
        }
        assert_eq!("1s250ms", print_age_precise(1250, true));
    }

    #[test]
    fn test_reverse() {
        let time = 259032000;
        assert_eq!(
            Ok(time),
            to_milli_seconds(&print_duration_precise(time, true))
        );
    }

    #[test]
    fn test_to_duration_millis() {
        assert_eq!(Ok(SignedDuration::of_millis(20000)), to_duration("20000"));
    }

    #[test]
    fn test_to_duration_human_readable() {
        assert_eq!(Ok(SignedDuration::of_seconds(20)), to_duration("20s"));
    }

    #[test]
    fn test_to_duration_human_readable_minutes() {
        assert_eq!(Ok(SignedDuration::of_seconds(90)), to_duration("1m30s"));
    }

    #[test]
    fn test_to_duration_iso8601() {
        assert_eq!(Ok(SignedDuration::of_seconds(20)), to_duration("PT20S"));
    }

    #[test]
    fn test_to_duration_iso8601_lowercase() {
        assert_eq!(Ok(SignedDuration::of_seconds(20)), to_duration("pt20s"));
    }

    #[test]
    fn test_to_duration_iso8601_negative() {
        assert_eq!(Ok(SignedDuration::of_seconds(-30)), to_duration("-PT30S"));
    }

    #[test]
    fn test_invalid_patterns() {
        for s in ["-", "x", "1x", "1h 2", "PT", "P"] {
            assert!(to_duration(s).is_err(), "{s}");
        }
        assert_eq!(Ok(0), to_milli_seconds(""));
        assert!(to_milli_seconds("99999999999999999999").is_err());
        assert!(to_milli_seconds("999999999999d").is_err());
        assert_eq!(Ok(5_400_000), to_milli_seconds(" 1h 30m "));
    }

    #[test]
    fn test_signed_duration() {
        let d = SignedDuration::of_millis(-1500);
        assert_eq!(-2, d.seconds());
        assert_eq!(500_000_000, d.nano());
        assert_eq!(-1500, d.to_millis());
        assert!(d.to_std().is_none());
        assert_eq!(
            Some(Duration::from_millis(1500)),
            SignedDuration::of_millis(1500).to_std()
        );
        assert!(is_positive(&SignedDuration::of_nanos(1)));
        assert!(!is_positive(&SignedDuration::ZERO));
        assert!(!is_positive(&SignedDuration::of_millis(-1)));
        assert_eq!("PT20S", SignedDuration::of_seconds(20).to_string());
        assert_eq!("PT1M30.5S", SignedDuration::of_millis(90_500).to_string());
        assert_eq!("PT0S", SignedDuration::ZERO.to_string());
        assert_eq!("PT1H", SignedDuration::of_seconds(3600).to_string());
        assert_eq!("PT-30S", SignedDuration::of_seconds(-30).to_string());
        assert_eq!(
            SignedDuration::of_millis(1500),
            SignedDuration::from(Duration::from_millis(1500))
        );
        assert_eq!(
            SignedDuration::of_millis(-500),
            SignedDuration::parse("PT-0.5S").unwrap()
        );
    }

    #[test]
    #[allow(deprecated)]
    fn test_since() {
        let now = now_millis();
        assert!(elapsed_millis_since(now) >= 0);
        assert_eq!("0ms", print_since(now + 10_000));
        assert!(!print_since_precise(now - 2000, true).is_empty());
    }
}
