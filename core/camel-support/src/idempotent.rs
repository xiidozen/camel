//! Idempotent repositories (Java `org.apache.camel.support.processor.idempotent`).

use camel_api::spi::idempotent_repository::IdempotentRepository;
use indexmap::IndexSet;
use std::sync::Mutex;

/// The default maximum number of keys of a [`MemoryIdempotentRepository`].
pub const DEFAULT_MAX_CACHE_SIZE: usize = 1000;

/// An idempotent repository in memory (Java `MemoryIdempotentRepository`), keeping at most
/// `max_cache_size` keys: the oldest keys are evicted first.
///
/// # Examples
///
/// ```
/// use camel_api::spi::idempotent_repository::IdempotentRepository;
/// use camel_support::idempotent::MemoryIdempotentRepository;
///
/// let repo = MemoryIdempotentRepository::with_max_cache_size(2);
/// assert!(repo.add("a"));
/// assert!(!repo.add("a"));
/// repo.add("b");
/// repo.add("c");
/// assert!(!repo.contains("a"), "the oldest key is evicted");
/// assert_eq!(2, repo.size());
/// ```
#[derive(Debug)]
pub struct MemoryIdempotentRepository {
    keys: Mutex<IndexSet<String>>,
    max_cache_size: usize,
}

impl Default for MemoryIdempotentRepository {
    fn default() -> Self {
        Self::with_max_cache_size(DEFAULT_MAX_CACHE_SIZE)
    }
}

impl MemoryIdempotentRepository {
    /// A repository of at most 1000 keys (Java `memoryIdempotentRepository()`).
    pub fn new() -> Self {
        Self::default()
    }

    /// A repository of at most the number of keys.
    pub fn with_max_cache_size(max_cache_size: usize) -> Self {
        MemoryIdempotentRepository {
            keys: Mutex::new(IndexSet::new()),
            max_cache_size,
        }
    }

    /// The number of keys.
    pub fn size(&self) -> usize {
        self.keys.lock().unwrap().len()
    }

    /// The maximum number of keys.
    pub fn max_cache_size(&self) -> usize {
        self.max_cache_size
    }
}

impl IdempotentRepository for MemoryIdempotentRepository {
    fn add(&self, key: &str) -> bool {
        let mut keys = self.keys.lock().unwrap();
        if keys.contains(key) {
            return false;
        }
        keys.insert(key.to_string());
        while self.max_cache_size > 0 && keys.len() > self.max_cache_size {
            keys.shift_remove_index(0);
        }
        true
    }

    fn contains(&self, key: &str) -> bool {
        self.keys.lock().unwrap().contains(key)
    }

    fn remove(&self, key: &str) -> bool {
        self.keys.lock().unwrap().shift_remove(key)
    }

    fn clear(&self) {
        self.keys.lock().unwrap().clear();
    }
}

/// A new in-memory repository of at most 1000 keys, shared (Java
/// `MemoryIdempotentRepository.memoryIdempotentRepository()`).
pub fn memory_idempotent_repository(
) -> camel_api::spi::idempotent_repository::SharedIdempotentRepository {
    std::sync::Arc::new(MemoryIdempotentRepository::new())
}

/// The repository as a value, to bind it in the registry.
pub fn repository_value(
    repository: camel_api::spi::idempotent_repository::SharedIdempotentRepository,
) -> camel_util::value::Value {
    camel_util::value::Value::Object(camel_util::value::ObjectRef::new(repository))
}

/// The repository held by a value of the registry (see [`repository_value`]).
pub fn repository_of(
    value: &camel_util::value::Value,
) -> Option<camel_api::spi::idempotent_repository::SharedIdempotentRepository> {
    match value {
        camel_util::value::Value::Object(o) => o
            .downcast_ref::<camel_api::spi::idempotent_repository::SharedIdempotentRepository>()
            .cloned(),
        _ => None,
    }
}
