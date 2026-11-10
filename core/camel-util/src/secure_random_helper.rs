//! Shared cryptographically secure randomness. Port of `org.apache.camel.util.SecureRandomHelper`.
//!
//! The Java class shared one `SecureRandom` instance. In Rust the operating system random
//! source is used directly (through the `getrandom` crate), which is thread-safe and needs no
//! initialization.
//!
//! This is intended for internal Camel code that needs "give me some randomness" without
//! caring about a specific algorithm or provider.

/// Fills the buffer with cryptographically secure random bytes.
///
/// # Panics
///
/// Panics if the operating system random source is not available.
pub fn fill_bytes(buf: &mut [u8]) {
    getrandom::fill(buf).expect("operating system random source unavailable");
}

/// Returns a cryptographically secure random `u64`.
pub fn next_u64() -> u64 {
    let mut buf = [0u8; 8];
    fill_bytes(&mut buf);
    u64::from_le_bytes(buf)
}

/// Returns a cryptographically secure random number in `0..bound` (without modulo bias).
///
/// # Panics
///
/// Panics if `bound` is zero.
pub fn next_below(bound: u64) -> u64 {
    assert!(bound > 0, "bound must be positive");
    let zone = u64::MAX - (u64::MAX % bound);
    loop {
        let v = next_u64();
        if v < zone {
            return v % bound;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random() {
        assert_ne!(next_u64(), next_u64());
        for _ in 0..100 {
            assert!(next_below(10) < 10);
        }
        let mut buf = [0u8; 32];
        fill_bytes(&mut buf);
        assert!(buf.iter().any(|&b| b != 0));
    }
}
