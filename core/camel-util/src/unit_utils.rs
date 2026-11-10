//! Unit utils. Port of `org.apache.camel.util.UnitUtils`.

/// Formats a size in bytes in a human friendly format with `xx kB`, `xx MB`, `xx GB` instead of
/// a large byte number (using units of 1000 and `.` as decimal separator).
///
/// The Java version used the decimal separator of the default locale; the Rust port always
/// uses `.`. Use [`print_unit_from_bytes_with`] for another separator.
///
/// # Arguments
///
/// * `bytes` - the value in bytes (a negative value gives an empty string)
///
/// # Examples
///
/// ```
/// use camel_util::unit_utils::print_unit_from_bytes;
/// assert_eq!("999 B", print_unit_from_bytes(999));
/// assert_eq!("1.2 kB", print_unit_from_bytes(1201));
/// assert_eq!("1.5 MB", print_unit_from_bytes(1500001));
/// ```
pub fn print_unit_from_bytes(bytes: i64) -> String {
    print_unit_from_bytes_with(bytes, '.')
}

/// Formats a size in bytes in a human friendly format, using dot as decimal separator.
///
/// # Arguments
///
/// * `bytes` - the value in bytes
pub fn print_unit_from_bytes_dot(bytes: i64) -> String {
    print_unit_from_bytes_with(bytes, '.')
}

/// Formats a size in bytes in a human friendly format with `xx kB`, `xx MB`, `xx GB`.
///
/// # Arguments
///
/// * `bytes` - the value in bytes (a negative value gives an empty string)
/// * `decimal` - the decimal separator char to use
pub fn print_unit_from_bytes_with(bytes: i64, decimal: char) -> String {
    if bytes < 0 {
        return String::new();
    }
    let unit = 1000f64;
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let exp = ((bytes as f64).ln() / unit.ln()) as i32;
    let pre = "kMGTPE".chars().nth((exp - 1) as usize).unwrap();
    let value = bytes as f64 / unit.powi(exp);
    // Java's %.1f rounds HALF_UP
    let rounded = (value * 10.0 + 0.5).floor() / 10.0;
    let answer = format!("{rounded:.1} {pre}B");
    if decimal == '.' {
        answer
    } else {
        answer.replace('.', &decimal.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_unit_from_bytes() {
        assert_eq!("999 B", print_unit_from_bytes(999));
        assert_eq!("1.0 kB", print_unit_from_bytes(1000));
        assert_eq!("1.0 kB", print_unit_from_bytes(1001));
        assert_eq!("1.2 kB", print_unit_from_bytes(1201));
        assert_eq!("1000.0 kB", print_unit_from_bytes(999999));
        assert_eq!("1.0 MB", print_unit_from_bytes(1000000));
        assert_eq!("1.0 MB", print_unit_from_bytes(1000001));
        assert_eq!("1.5 MB", print_unit_from_bytes(1500001));
        assert_eq!("", print_unit_from_bytes(-1));
    }

    #[test]
    fn test_print_unit_from_bytes_dot() {
        assert_eq!("999 B", print_unit_from_bytes_dot(999));
        assert_eq!("1.0 kB", print_unit_from_bytes_dot(1000));
        assert_eq!("1.0 kB", print_unit_from_bytes_dot(1001));
        assert_eq!("1.2 kB", print_unit_from_bytes_dot(1201));
        assert_eq!("1000.0 kB", print_unit_from_bytes_dot(999999));
        assert_eq!("1.0 MB", print_unit_from_bytes_dot(1000000));
        assert_eq!("1.0 MB", print_unit_from_bytes_dot(1000001));
        assert_eq!("1.5 MB", print_unit_from_bytes_dot(1500001));
    }

    #[test]
    fn test_print_unit_from_bytes_comma() {
        assert_eq!("1,5 MB", print_unit_from_bytes_with(1500001, ','));
    }
}
