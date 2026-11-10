//! The UUID generators (Java `ClassicUuidGenerator`, `ShortUuidGenerator`,
//! `SimpleUuidGenerator`, `RandomUuidGenerator` and `OffUuidGenerator`). The default generator
//! is `camel_api::spi::uuid::DefaultUuidGenerator`.

use camel_api::spi::uuid::{DefaultUuidGenerator, UuidGenerator};
use camel_util::secure_random_helper::{fill_bytes, next_u64};
use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Generates `1`, `2`, `3`, ... (for testing).
#[derive(Debug)]
pub struct SimpleUuidGenerator {
    id: AtomicI64,
}

impl Default for SimpleUuidGenerator {
    fn default() -> Self {
        SimpleUuidGenerator {
            id: AtomicI64::new(1),
        }
    }
}

impl UuidGenerator for SimpleUuidGenerator {
    fn generate_uuid(&self) -> String {
        self.id.fetch_add(1, Ordering::SeqCst).to_string()
    }
}

/// Generates random UUIDs (version 4, Java `UUID.randomUUID()`).
#[derive(Debug, Default)]
pub struct RandomUuidGenerator;

/// A random UUID in the standard text form.
///
/// # Examples
///
/// ```
/// let u = camel_support::uuid_generators::random_uuid();
/// assert_eq!(36, u.len());
/// assert_eq!(Some('4'), u.chars().nth(14));
/// ```
pub fn random_uuid() -> String {
    let mut b = [0u8; 16];
    fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

impl UuidGenerator for RandomUuidGenerator {
    fn generate_uuid(&self) -> String {
        random_uuid()
    }
}

/// Generates short ids: a random seed and a counter in hex, such as `B5F2A-000000000A`.
#[derive(Debug)]
pub struct ShortUuidGenerator {
    seed: String,
    index: AtomicU64,
}

impl Default for ShortUuidGenerator {
    fn default() -> Self {
        let mut v = next_u64();
        let mut hex = [0u8; 6];
        for j in (0..6).rev() {
            hex[j] = HEX[(v & 0x0f) as usize];
            v >>= 4;
        }
        let seed = format!("{}-", std::str::from_utf8(&hex[1..]).unwrap_or("00000"));
        ShortUuidGenerator {
            seed,
            index: AtomicU64::new(0),
        }
    }
}

impl UuidGenerator for ShortUuidGenerator {
    fn generate_uuid(&self) -> String {
        let mut v = self.index.fetch_add(1, Ordering::SeqCst);
        let mut digits = [0u8; 10];
        for j in (0..10).rev() {
            digits[j] = HEX[(v & 0x0f) as usize];
            v >>= 4;
        }
        format!(
            "{}{}",
            self.seed,
            std::str::from_utf8(&digits).unwrap_or("")
        )
    }
}

static CLASSIC_INSTANCES: AtomicU32 = AtomicU32::new(0);

fn classic_stub() -> &'static str {
    static STUB: OnceLock<String> = OnceLock::new();
    STUB.get_or_init(|| {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        format!("-{millis}-")
    })
}

/// The host name with non ASCII characters removed (Java `sanitizeHostName`).
pub fn sanitize_host_name(host_name: &str) -> String {
    host_name.chars().filter(|c| (*c as u32) < 127).collect()
}

/// The id with `:`, `_`, `.` and `/` replaced by `-` (Java `generateSanitizedId`).
pub fn generate_sanitized_id(id: &str) -> String {
    id.replace([':', '_', '.', '/'], "-")
}

/// Generates `ID-<host>-<start millis>-<instance>-<sequence>` ids (Java
/// `ClassicUuidGenerator`).
#[derive(Debug)]
pub struct ClassicUuidGenerator {
    seed: String,
    sequence: AtomicI64,
}

impl Default for ClassicUuidGenerator {
    fn default() -> Self {
        let host = camel_util::host_utils::get_local_host_name_safe()
            .unwrap_or_else(|| "localhost".to_string());
        Self::with_prefix(&format!("ID-{}", sanitize_host_name(&host)))
    }
}

impl ClassicUuidGenerator {
    /// A generator with the id prefix.
    pub fn with_prefix(prefix: &str) -> Self {
        let instance = CLASSIC_INSTANCES.fetch_add(1, Ordering::SeqCst);
        let seed = generate_sanitized_id(&format!("{prefix}{}{instance}-", classic_stub()));
        ClassicUuidGenerator {
            seed,
            sequence: AtomicI64::new(1),
        }
    }
}

impl UuidGenerator for ClassicUuidGenerator {
    fn generate_uuid(&self) -> String {
        format!(
            "{}{}",
            self.seed,
            self.sequence.fetch_add(1, Ordering::SeqCst)
        )
    }
}

/// The default generator, but exchanges get an empty id (Java `OffUuidGenerator`, for best
/// performance when exchange ids are not needed).
#[derive(Debug, Default)]
pub struct OffUuidGenerator(DefaultUuidGenerator);

impl UuidGenerator for OffUuidGenerator {
    fn generate_uuid(&self) -> String {
        self.0.generate_uuid()
    }

    fn generate_exchange_uuid(&self) -> String {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generators() {
        let s = SimpleUuidGenerator::default();
        assert_eq!(
            ("1".to_string(), "2".to_string()),
            (s.generate_uuid(), s.generate_uuid())
        );
        let short = ShortUuidGenerator::default();
        let a = short.generate_uuid();
        assert_eq!(16, a.len());
        assert!(a.ends_with("0000000000"));
        assert!(short.generate_uuid().ends_with("0000000001"));
        assert_ne!(random_uuid(), RandomUuidGenerator.generate_uuid());
        let c = ClassicUuidGenerator::with_prefix("ID-my.host");
        let id = c.generate_uuid();
        assert!(id.starts_with("ID-my-host-") && id.ends_with("-1"), "{id}");
        assert_eq!("ab", sanitize_host_name("aéb"));
        assert_eq!("", OffUuidGenerator::default().generate_exchange_uuid());
        assert!(!OffUuidGenerator::default().generate_uuid().is_empty());
    }
}
