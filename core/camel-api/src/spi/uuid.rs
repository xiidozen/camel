//! Generator of unique ids (Java `org.apache.camel.spi.UuidGenerator` and the default
//! implementation `org.apache.camel.support.DefaultUuidGenerator`).

use std::sync::atomic::{AtomicU64, Ordering};

/// Generator of unique ids for exchanges, messages and other objects.
pub trait UuidGenerator: Send + Sync {
    /// Generates a unique id.
    fn generate_uuid(&self) -> String;

    /// Generates a unique id for an exchange (by default [`UuidGenerator::generate_uuid`]).
    fn generate_exchange_uuid(&self) -> String {
        self.generate_uuid()
    }
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

fn long_to_hex(prefix: &str, v: u64) -> String {
    let mut out = String::with_capacity(prefix.len() + 16);
    out.push_str(prefix);
    for j in (0..16).rev() {
        out.push(HEX[((v >> (j * 4)) & 0x0F) as usize] as char);
    }
    out
}

/// The default generator: a random 15 hex digit seed, a dash and a 16 hex digit counter (32
/// characters), such as `5E2D1B0A9F31C4D-0000000000000001`.
///
/// # Examples
///
/// ```
/// use camel_api::spi::uuid::{DefaultUuidGenerator, UuidGenerator};
/// let g = DefaultUuidGenerator::new();
/// let a = g.generate_uuid();
/// let b = g.generate_uuid();
/// assert_eq!(32, a.len());
/// assert_ne!(a, b);
/// assert!(a.ends_with("-0000000000000000"));
/// ```
#[derive(Debug)]
pub struct DefaultUuidGenerator {
    seed: String,
    index: AtomicU64,
}

impl Default for DefaultUuidGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultUuidGenerator {
    /// Creates the generator with a random seed.
    pub fn new() -> Self {
        let seed = long_to_hex("", camel_util::secure_random_helper::next_u64());
        DefaultUuidGenerator {
            seed: format!("{}-", &seed[1..]),
            index: AtomicU64::new(0),
        }
    }
}

impl UuidGenerator for DefaultUuidGenerator {
    fn generate_uuid(&self) -> String {
        long_to_hex(&self.seed, self.index.fetch_add(1, Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format() {
        let g = DefaultUuidGenerator::new();
        let id = g.generate_exchange_uuid();
        assert_eq!(32, id.len());
        assert_eq!(Some(15), id.find('-'));
        assert!(id.ends_with("0000000000000000"));
        assert!(g.generate_uuid().ends_with("0000000000000001"));
        assert!(id.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
    }
}
