//! The repository of the inflight exchanges (Java `InflightRepository` and
//! `DefaultInflightRepository`), a plugin of the context: the exchanges being routed, in total
//! (from the creation to the end of their unit of work) and per route (while in the route).
//!
//! The Rust exchanges are values, so browsing gives snapshots ([`InflightExchange`]) of the
//! exchanges when added, with their duration so far.

use crate::exchange::Exchange;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// An inflight exchange (Java `InflightRepository.InflightExchange`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InflightExchange {
    /// The id of the exchange.
    pub exchange_id: String,
    /// The id of the route the exchange was created by, if any.
    pub from_route_id: Option<String>,
    /// The id of the route the exchange is in, if any.
    pub at_route_id: Option<String>,
    /// The millis since the exchange was added.
    pub duration: u64,
}

/// The inflight exchanges of a context (Java `DefaultInflightRepository`).
#[derive(Debug, Default)]
pub struct InflightRepository {
    size: AtomicI64,
    routes: Mutex<HashMap<String, i64>>,
    browse_enabled: AtomicBool,
    inflight: Mutex<HashMap<String, Entry>>,
}

/// A browsed exchange: the route it was created by, when it was added, the routes it is in.
#[derive(Debug)]
struct Entry {
    from_route_id: Option<String>,
    added: Instant,
    routes: Vec<String>,
}

impl Entry {
    fn of(exchange: &Exchange) -> Self {
        Entry {
            from_route_id: exchange.from_route_id().map(str::to_string),
            added: Instant::now(),
            routes: Vec::new(),
        }
    }
}

impl InflightRepository {
    /// Adds the exchange (its unit of work begins).
    pub fn add(&self, exchange: &Exchange) {
        self.size.fetch_add(1, Ordering::SeqCst);
        if self.is_inflight_browse_enabled() {
            self.inflight
                .lock()
                .unwrap()
                .entry(exchange.exchange_id().to_string())
                .or_insert_with(|| Entry::of(exchange));
        }
    }

    /// Removes the exchange (its unit of work is done).
    pub fn remove(&self, exchange: &Exchange) {
        self.size.fetch_sub(1, Ordering::SeqCst);
        if self.is_inflight_browse_enabled() {
            self.inflight.lock().unwrap().remove(exchange.exchange_id());
        }
    }

    /// The exchange enters the route (counted for a route added with
    /// [`add_route`](Self::add_route)).
    pub fn add_to_route(&self, exchange: &Exchange, route_id: &str) {
        if let Some(n) = self.routes.lock().unwrap().get_mut(route_id) {
            *n += 1;
        }
        if self.is_inflight_browse_enabled() {
            // the route counts the exchange before its unit of work adds it
            self.inflight
                .lock()
                .unwrap()
                .entry(exchange.exchange_id().to_string())
                .or_insert_with(|| Entry::of(exchange))
                .routes
                .push(route_id.to_string());
        }
    }

    /// The exchange leaves the route.
    pub fn remove_from_route(&self, exchange: &Exchange, route_id: &str) {
        if let Some(n) = self.routes.lock().unwrap().get_mut(route_id) {
            *n -= 1;
        }
        if self.is_inflight_browse_enabled() {
            if let Some(e) = self
                .inflight
                .lock()
                .unwrap()
                .get_mut(exchange.exchange_id())
            {
                e.routes.pop();
            }
        }
    }

    /// The number of inflight exchanges.
    pub fn size(&self) -> usize {
        self.size.load(Ordering::SeqCst).max(0) as usize
    }

    /// Counts the exchanges of the route (Java `addRoute`).
    pub fn add_route(&self, route_id: &str) {
        self.routes
            .lock()
            .unwrap()
            .entry(route_id.to_string())
            .or_insert(0);
    }

    /// No longer counts the exchanges of the route (Java `removeRoute`).
    pub fn remove_route(&self, route_id: &str) {
        self.routes.lock().unwrap().remove(route_id);
    }

    /// The number of exchanges in the route (Java `size(routeId)`).
    pub fn route_size(&self, route_id: &str) -> usize {
        self.routes
            .lock()
            .unwrap()
            .get(route_id)
            .map(|n| (*n).max(0) as usize)
            .unwrap_or(0)
    }

    /// Whether the exchanges can be browsed (default `false`).
    pub fn is_inflight_browse_enabled(&self) -> bool {
        self.browse_enabled.load(Ordering::SeqCst)
    }

    /// Sets whether the exchanges can be browsed.
    pub fn set_inflight_browse_enabled(&self, enabled: bool) {
        self.browse_enabled.store(enabled, Ordering::SeqCst);
    }

    /// The inflight exchanges (of the route they were created by, if given), at most `limit`
    /// (if positive), sorted by exchange id or by longest duration (Java `browse`); empty
    /// unless browsing is enabled.
    pub fn browse(
        &self,
        from_route_id: Option<&str>,
        limit: i32,
        sort_by_longest_duration: bool,
    ) -> Vec<InflightExchange> {
        if !self.is_inflight_browse_enabled() {
            return Vec::new();
        }
        let mut list: Vec<InflightExchange> = self
            .inflight
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, e)| from_route_id.is_none() || e.from_route_id.as_deref() == from_route_id)
            .map(|(id, e)| InflightExchange {
                exchange_id: id.clone(),
                from_route_id: e.from_route_id.clone(),
                at_route_id: e.routes.last().cloned(),
                duration: e.added.elapsed().as_millis() as u64,
            })
            .collect();
        if sort_by_longest_duration {
            list.sort_by_key(|e| std::cmp::Reverse(e.duration));
        } else {
            list.sort_by(|a, b| a.exchange_id.cmp(&b.exchange_id));
        }
        if limit > 0 {
            list.truncate(limit as usize);
        }
        list
    }

    /// The oldest inflight exchange (of the route), if browsing is enabled (Java `oldest`).
    pub fn oldest(&self, from_route_id: Option<&str>) -> Option<InflightExchange> {
        self.browse(from_route_id, 1, true).into_iter().next()
    }
}
