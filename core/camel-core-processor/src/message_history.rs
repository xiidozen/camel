//! Records the message history (Java `CamelInternalProcessor.MessageHistoryAdvice`): an entry
//! per step, with the time it took.

use camel_api::error::CamelResult;
use camel_api::exchange::Exchange;
use camel_api::message_history::{
    add_message_history, body_size, body_type, set_message_history_elapsed, MessageHistory,
};
use camel_api::processor::{BoxFuture, Processor, SharedProcessor};
use std::time::Instant;

/// The node of a step in the history.
#[derive(Debug, Clone)]
pub struct HistoryNode {
    /// The route id.
    pub route_id: String,
    /// The node id.
    pub node_id: String,
    /// The short name, such as `to`.
    pub short_name: String,
    /// The label, such as `to[mock:a]`.
    pub label: String,
    /// Whether the body size is computed.
    pub size_enabled: bool,
    /// Whether the entry keeps a copy of the message.
    pub copy_message: bool,
}

/// Adds an entry to the message history of the exchange, then runs the step.
pub struct MessageHistoryProcessor {
    node: HistoryNode,
    target: SharedProcessor,
}

impl MessageHistoryProcessor {
    /// Wraps the processor of the step.
    pub fn new(node: HistoryNode, target: SharedProcessor) -> Self {
        MessageHistoryProcessor { node, target }
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

impl Processor for MessageHistoryProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        self.target.start_processor()
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        self.target.stop_processor()
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let body = exchange.message().body();
            let entry = MessageHistory {
                route_id: self.node.route_id.clone(),
                node_id: self.node.node_id.clone(),
                short_name: self.node.short_name.clone(),
                label: self.node.label.clone(),
                time: now_millis(),
                elapsed: 0,
                body_type: Some(body_type(body)),
                body_size: if self.node.size_enabled && !body.is_null() {
                    body_size(body)
                } else {
                    -1
                },
                message: self
                    .node
                    .copy_message
                    .then(|| Box::new(exchange.message().clone())),
            };
            let index = add_message_history(exchange, entry);
            let started = Instant::now();
            let result = self.target.process(exchange).await;
            set_message_history_elapsed(exchange, index, started.elapsed().as_millis() as i64);
            result
        })
    }
}
