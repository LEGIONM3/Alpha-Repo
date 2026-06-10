//! Subscription management for the Event Bus.
//!
//! Each subscription is a (pattern, handler) pair. When an event is published,
//! all subscribers whose pattern matches the event's topic are invoked.

use std::sync::Arc;

use alpha_common::event::Event;
use alpha_common::types::AlphaId;

/// Opaque handle for a subscription. Used to unsubscribe.
#[derive(Debug, Clone)]
pub struct SubscriptionHandle {
    /// Unique subscription ID.
    pub id: AlphaId,
    /// The topic pattern this subscription matches.
    pub pattern: String,
}

/// Type-erased event handler function.
pub(crate) type HandlerFn = Arc<dyn Fn(Event) + Send + Sync>;

/// An active subscription: pattern + handler.
pub(crate) struct Subscription {
    pub handle: SubscriptionHandle,
    pub handler: HandlerFn,
}
