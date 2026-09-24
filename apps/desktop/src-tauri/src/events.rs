//! Per-session event ring. Events are invalidation hints; the desktop engine
//! publishes only scan lifecycle transitions today.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde::Serialize;

pub const MAX_EVENTS: usize = 1024;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventEnvelope {
    pub sequence: u64,
    pub emitted_at: String,
    pub payload: serde_json::Value,
}

/// The host mints subscription ids — the caller never picks one. (Learned the
/// hard way in a sibling project: client-minted ids drift out of sync with
/// host-minted frames and live events silently stop matching.)
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionAck {
    pub subscription_id: String,
    pub service_instance_id: String,
    pub high_watermark: u64,
    pub replay: Vec<EventEnvelope>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScopedEventFrame {
    pub session_id: String,
    pub subscription_id: String,
    pub service_instance_id: String,
    pub event: EventEnvelope,
}

#[derive(Default)]
struct Inner {
    sequence: u64,
    ring: VecDeque<EventEnvelope>,
}

#[derive(Clone, Default)]
pub struct EventSink {
    inner: Arc<Mutex<Inner>>,
}

impl EventSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish(&self, payload: serde_json::Value) -> Option<EventEnvelope> {
        let mut inner = self.inner.lock().unwrap();
        inner.sequence += 1;
        let envelope = EventEnvelope {
            sequence: inner.sequence,
            emitted_at: now_iso(),
            payload,
        };
        if inner.ring.len() >= MAX_EVENTS {
            inner.ring.pop_front();
        }
        inner.ring.push_back(envelope.clone());
        Some(envelope)
    }

    pub fn subscribe(&self, after_sequence: Option<u64>) -> (String, SubscriptionAck) {
        let inner = self.inner.lock().unwrap();
        let subscription_id = format!("sub_{}", inner.sequence + 1);
        let mut replay = Vec::new();
        match after_sequence {
            Some(since) => {
                replay.extend(inner.ring.iter().filter(|envelope| envelope.sequence > since).cloned());
            }
            None => replay.extend(inner.ring.iter().cloned()),
        }
        let ack = SubscriptionAck {
            subscription_id,
            service_instance_id: String::new(),
            high_watermark: inner.sequence,
            replay,
        };
        (ack.subscription_id.clone(), ack)
    }

    pub fn frames_for(&self, session_id: &str, subscription_id: &str, service_instance_id: &str) -> Vec<ScopedEventFrame> {
        let inner = self.inner.lock().unwrap();
        inner
            .ring
            .iter()
            .map(|event| ScopedEventFrame {
                session_id: session_id.to_string(),
                subscription_id: subscription_id.to_string(),
                service_instance_id: service_instance_id.to_string(),
                event: event.clone(),
            })
            .collect()
    }
}

fn now_iso() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("{}.{:03}Z", now.as_secs(), now.subsec_millis())
}
