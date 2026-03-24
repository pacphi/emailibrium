//! Domain event system for Emailibrium (Audit Item #20).
//!
//! Provides an in-process event bus backed by Tokio broadcast channels.
//! Every bounded context publishes domain events instead of relying on
//! direct `Arc` function calls, enabling loose coupling and future
//! event-sourcing capabilities.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Domain Event Envelope
// ---------------------------------------------------------------------------

/// Metadata common to all domain events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// Unique identifier for this event instance.
    pub event_id: Uuid,
    /// When the event was created.
    pub timestamp: DateTime<Utc>,
    /// The aggregate that produced the event (e.g. email_id, account_id).
    pub aggregate_id: String,
    /// Human-readable event type name.
    pub event_type: String,
    /// The actual event payload.
    pub payload: DomainEvent,
}

impl EventEnvelope {
    /// Create a new envelope wrapping the given event.
    pub fn new(aggregate_id: impl Into<String>, payload: DomainEvent) -> Self {
        Self {
            event_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            aggregate_id: aggregate_id.into(),
            event_type: payload.event_type().to_string(),
            payload,
        }
    }
}

// ---------------------------------------------------------------------------
// Domain Events
// ---------------------------------------------------------------------------

/// All domain events across bounded contexts.
///
/// Each variant maps to a specific domain occurrence described in the DDDs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DomainEvent {
    // -- DDD-001: Email Intelligence ----------------------------------------
    /// An email was ingested into the system and persisted.
    EmailIngested {
        email_id: String,
        account_id: String,
        subject: String,
        from_addr: String,
    },

    /// An email's text was embedded into a vector.
    EmailEmbedded {
        email_id: String,
        vector_id: String,
        model: String,
        dimensions: usize,
    },

    /// An email was classified into a category.
    EmailClassified {
        email_id: String,
        category: String,
        confidence: f32,
        method: String,
    },

    /// A new cluster was discovered by the clustering engine.
    ClusterDiscovered {
        cluster_id: String,
        email_count: u64,
        representative_subject: String,
    },

    // -- DDD-005: Account Management ----------------------------------------
    /// An email account was connected via OAuth.
    AccountConnected {
        account_id: String,
        provider: String,
        email_address: String,
    },

    /// An account sync completed.
    AccountSynced {
        account_id: String,
        emails_synced: u64,
        failures: u32,
    },

    /// An account's health status changed.
    AccountHealthChanged {
        account_id: String,
        old_status: String,
        new_status: String,
        reason: String,
    },

    // -- DDD-006: AI Providers ----------------------------------------------
    /// The active generative model was switched.
    ModelSwitched {
        old_provider: String,
        new_provider: String,
        reason: String,
    },

    /// A model's lifecycle state changed (downloaded, verified, quarantined).
    ModelLifecycleChanged {
        model_id: String,
        old_state: String,
        new_state: String,
    },

    /// An inference session started.
    InferenceSessionStarted {
        session_id: String,
        model: String,
        provider: String,
    },

    /// An inference session completed.
    InferenceSessionCompleted {
        session_id: String,
        tokens_used: u32,
        latency_ms: u64,
        success: bool,
    },
}

impl DomainEvent {
    /// Return the canonical event type name for logging and routing.
    pub fn event_type(&self) -> &'static str {
        match self {
            DomainEvent::EmailIngested { .. } => "email.ingested",
            DomainEvent::EmailEmbedded { .. } => "email.embedded",
            DomainEvent::EmailClassified { .. } => "email.classified",
            DomainEvent::ClusterDiscovered { .. } => "cluster.discovered",
            DomainEvent::AccountConnected { .. } => "account.connected",
            DomainEvent::AccountSynced { .. } => "account.synced",
            DomainEvent::AccountHealthChanged { .. } => "account.health_changed",
            DomainEvent::ModelSwitched { .. } => "model.switched",
            DomainEvent::ModelLifecycleChanged { .. } => "model.lifecycle_changed",
            DomainEvent::InferenceSessionStarted { .. } => "inference.session_started",
            DomainEvent::InferenceSessionCompleted { .. } => "inference.session_completed",
        }
    }
}

// ---------------------------------------------------------------------------
// Event Bus
// ---------------------------------------------------------------------------

/// Event handler callback type.
pub type EventHandler = Arc<dyn Fn(&EventEnvelope) + Send + Sync>;

/// In-process event bus using Tokio broadcast channels.
///
/// Supports publish/subscribe semantics. Subscribers receive cloned events.
/// The bus is designed for moderate throughput (hundreds of events/sec);
/// for high-throughput scenarios, consider batching.
#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<EventEnvelope>,
    /// Registered synchronous handlers invoked on publish (for logging, metrics).
    handlers: Arc<RwLock<Vec<EventHandler>>>,
}

impl EventBus {
    /// Create a new event bus with the given channel capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            handlers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create an event bus with a default capacity of 256.
    pub fn default_capacity() -> Self {
        Self::new(256)
    }

    /// Publish a domain event to all subscribers.
    ///
    /// Returns the number of receivers that received the event.
    /// Returns 0 if there are no active subscribers (this is not an error).
    pub async fn publish(&self, event: EventEnvelope) -> usize {
        // Invoke synchronous handlers first (logging, metrics, etc.)
        let handlers = self.handlers.read().await;
        for handler in handlers.iter() {
            handler(&event);
        }
        drop(handlers);

        // Broadcast to async subscribers
        self.sender.send(event).unwrap_or(0)
    }

    /// Convenience: wrap a `DomainEvent` in an envelope and publish.
    pub async fn emit(&self, aggregate_id: impl Into<String>, event: DomainEvent) -> usize {
        let envelope = EventEnvelope::new(aggregate_id, event);
        self.publish(envelope).await
    }

    /// Subscribe to all events on this bus.
    pub fn subscribe(&self) -> broadcast::Receiver<EventEnvelope> {
        self.sender.subscribe()
    }

    /// Register a synchronous handler invoked on every publish.
    ///
    /// Useful for logging or metrics collection without spawning a subscriber task.
    pub async fn on_event(&self, handler: EventHandler) {
        let mut handlers = self.handlers.write().await;
        handlers.push(handler);
    }

    /// Return the current number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::default_capacity()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn test_publish_and_subscribe() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();

        let event = DomainEvent::EmailIngested {
            email_id: "e-1".into(),
            account_id: "acct-1".into(),
            subject: "Test".into(),
            from_addr: "test@example.com".into(),
        };

        let count = bus.emit("e-1", event.clone()).await;
        assert_eq!(count, 1);

        let received = rx.recv().await.unwrap();
        assert_eq!(received.aggregate_id, "e-1");
        assert_eq!(received.event_type, "email.ingested");

        match received.payload {
            DomainEvent::EmailIngested { email_id, .. } => {
                assert_eq!(email_id, "e-1");
            }
            _ => panic!("Wrong event variant"),
        }
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = EventBus::new(16);
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();

        let event = DomainEvent::EmailEmbedded {
            email_id: "e-1".into(),
            vector_id: "v-1".into(),
            model: "test-model".into(),
            dimensions: 384,
        };

        let count = bus.emit("e-1", event).await;
        assert_eq!(count, 2);

        let e1 = rx1.recv().await.unwrap();
        let e2 = rx2.recv().await.unwrap();
        assert_eq!(e1.event_id, e2.event_id);
    }

    #[tokio::test]
    async fn test_no_subscribers_returns_zero() {
        let bus = EventBus::new(16);

        let event = DomainEvent::EmailClassified {
            email_id: "e-1".into(),
            category: "Work".into(),
            confidence: 0.95,
            method: "vector_centroid".into(),
        };

        let count = bus.emit("e-1", event).await;
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_synchronous_handler() {
        let bus = EventBus::new(16);
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        bus.on_event(Arc::new(move |_event| {
            counter_clone.fetch_add(1, Ordering::Relaxed);
        }))
        .await;

        let event = DomainEvent::AccountConnected {
            account_id: "acct-1".into(),
            provider: "gmail".into(),
            email_address: "user@gmail.com".into(),
        };

        bus.emit("acct-1", event).await;
        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_event_envelope_fields() {
        let event = DomainEvent::ModelSwitched {
            old_provider: "ollama".into(),
            new_provider: "cloud".into(),
            reason: "load balancing".into(),
        };

        let envelope = EventEnvelope::new("model-1", event);
        assert_eq!(envelope.aggregate_id, "model-1");
        assert_eq!(envelope.event_type, "model.switched");
        assert!(envelope.timestamp <= Utc::now());
    }

    #[tokio::test]
    async fn test_event_type_names() {
        assert_eq!(
            DomainEvent::EmailIngested {
                email_id: String::new(),
                account_id: String::new(),
                subject: String::new(),
                from_addr: String::new(),
            }
            .event_type(),
            "email.ingested"
        );
        assert_eq!(
            DomainEvent::ClusterDiscovered {
                cluster_id: String::new(),
                email_count: 0,
                representative_subject: String::new(),
            }
            .event_type(),
            "cluster.discovered"
        );
        assert_eq!(
            DomainEvent::InferenceSessionCompleted {
                session_id: String::new(),
                tokens_used: 0,
                latency_ms: 0,
                success: false,
            }
            .event_type(),
            "inference.session_completed"
        );
    }

    #[tokio::test]
    async fn test_subscriber_count() {
        let bus = EventBus::new(16);
        assert_eq!(bus.subscriber_count(), 0);

        let _rx1 = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 1);

        let _rx2 = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 2);

        drop(_rx1);
        assert_eq!(bus.subscriber_count(), 1);
    }

    #[tokio::test]
    async fn test_default_bus() {
        let bus = EventBus::default();
        assert_eq!(bus.subscriber_count(), 0);
        // Verify it works
        let _rx = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 1);
    }
}
