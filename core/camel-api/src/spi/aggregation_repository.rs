//! The repository of the exchanges being aggregated (Java
//! `org.apache.camel.spi.AggregationRepository`).

use crate::exchange::Exchange;

/// Stores the aggregated exchange of each correlation key while the aggregation is in
/// progress. Implementations must be thread safe; the aggregator calls them under its lock.
pub trait AggregationRepository: Send + Sync {
    /// Adds (or replaces) the aggregated exchange of the key.
    ///
    /// # Returns
    ///
    /// The previous exchange of the key, if any.
    fn add(&self, key: &str, exchange: Exchange) -> Option<Exchange>;

    /// The aggregated exchange of the key, if any.
    fn get(&self, key: &str) -> Option<Exchange>;

    /// Removes the exchange of the key, as the aggregation is complete (the exchange is now
    /// being processed; see [`confirm`](AggregationRepository::confirm)).
    fn remove(&self, key: &str, exchange: &Exchange);

    /// Confirms the completed exchange has been processed.
    fn confirm(&self, exchange_id: &str);

    /// The correlation keys currently being aggregated.
    fn keys(&self) -> Vec<String>;
}
