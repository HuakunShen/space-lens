//! Per-session event ring. Events are invalidation hints; the desktop engine
//! publishes only scan lifecycle transitions today.

use std::collections::{HashMap, VecDeque};
use std::sync::{mpsc, Arc, Mutex};

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
  subscriptions: HashMap<String, mpsc::Sender<EventEnvelope>>,
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
    // Broadcast under the lock that registers subscriptions: a subscriber
    // registered before this publish is guaranteed to see it on its channel.
    // Senders whose receiver was dropped (forwarding thread gone) fall out.
    inner
      .subscriptions
      .retain(|_, sender| sender.send(envelope.clone()).is_ok());
    Some(envelope)
  }

  /// Registers a live channel under the lock first — every event published
  /// after this call reaches the receiver — then snapshots the replay ring
  /// filtered to `after_sequence`. Frames delivered both ways (published
  /// between registration and the snapshot) overlap by construction; the
  /// client's sequence watermark deduplicates them.
  pub fn subscribe_channel(
    &self,
    after_sequence: Option<u64>,
  ) -> (String, SubscriptionAck, mpsc::Receiver<EventEnvelope>) {
    let mut inner = self.inner.lock().unwrap();
    let (sender, receiver) = mpsc::channel();
    let subscription_id = format!("sub_{}", inner.sequence + 1);
    inner.subscriptions.insert(subscription_id.clone(), sender);
    let mut replay = Vec::new();
    match after_sequence {
      Some(since) => {
        replay.extend(
          inner
            .ring
            .iter()
            .filter(|envelope| envelope.sequence > since)
            .cloned(),
        );
      }
      None => replay.extend(inner.ring.iter().cloned()),
    }
    let ack = SubscriptionAck {
      subscription_id: subscription_id.clone(),
      service_instance_id: String::new(),
      high_watermark: inner.sequence,
      replay,
    };
    (subscription_id, ack, receiver)
  }

  /// Ack-only subscription for callers that replay explicitly and do not
  /// need a live channel.
  pub fn subscribe(&self, after_sequence: Option<u64>) -> (String, SubscriptionAck) {
    let (subscription_id, ack, receiver) = self.subscribe_channel(after_sequence);
    drop(receiver);
    (subscription_id, ack)
  }

  /// Removes the subscription sender; the connected forwarding thread's
  /// `recv` fails and the thread exits.
  pub fn unsubscribe(&self, subscription_id: &str) {
    self
      .inner
      .lock()
      .unwrap()
      .subscriptions
      .remove(subscription_id);
  }

  pub fn frames_for(
    &self,
    session_id: &str,
    subscription_id: &str,
    service_instance_id: &str,
  ) -> Vec<ScopedEventFrame> {
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
  let now = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap_or_default();
  format!("{}.{:03}Z", now.as_secs(), now.subsec_millis())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::time::Duration;

  const TIMEOUT: Duration = Duration::from_secs(2);

  fn payload(kind: &str) -> serde_json::Value {
    serde_json::json!({ "kind": kind })
  }

  #[test]
  fn channel_receives_everything_published_after_subscribing_in_order() {
    let sink = EventSink::new();
    sink.publish(payload("a"));
    let (_id, ack, receiver) = sink.subscribe_channel(None);
    assert_eq!(ack.high_watermark, 1);
    assert_eq!(ack.replay.len(), 1, "the ring replays prior events");
    sink.publish(payload("b"));
    sink.publish(payload("c"));
    let first = receiver.recv_timeout(TIMEOUT).unwrap();
    let second = receiver.recv_timeout(TIMEOUT).unwrap();
    assert_eq!(first.sequence, 2);
    assert_eq!(first.payload["kind"], "b");
    assert_eq!(second.sequence, 3);
    assert_eq!(second.payload["kind"], "c");
  }

  #[test]
  fn unsubscribe_stops_delivery_and_later_publishes_do_not_revive_it() {
    let sink = EventSink::new();
    let (id, _ack, receiver) = sink.subscribe_channel(None);
    sink.unsubscribe(&id);
    sink.publish(payload("after"));
    assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
    // The dropped subscription stays dropped even after more traffic.
    sink.publish(payload("again"));
    assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
  }

  #[test]
  fn after_sequence_filters_the_replay_but_not_the_live_channel() {
    let sink = EventSink::new();
    sink.publish(payload("one"));
    let kept = sink.publish(payload("two")).unwrap();
    sink.publish(payload("three"));
    let (_id, ack, receiver) = sink.subscribe_channel(Some(kept.sequence));
    assert_eq!(
      ack.replay.iter().map(|e| e.sequence).collect::<Vec<_>>(),
      vec![3],
      "only frames after the watermark replay"
    );
    sink.publish(payload("four"));
    assert_eq!(receiver.recv_timeout(TIMEOUT).unwrap().sequence, 4);
  }

  #[test]
  fn replay_and_channel_union_covers_every_event_exactly_once_per_side() {
    let sink = EventSink::new();
    sink.publish(payload("before-1"));
    sink.publish(payload("before-2"));
    let (_id, ack, receiver) = sink.subscribe_channel(Some(1));
    sink.publish(payload("after-3"));
    sink.publish(payload("after-4"));
    // Events before the subscribe appear only in the replay; events after
    // appear only on the channel. "before-1" sits at the watermark and is
    // intentionally outside this subscription's window; everything from the
    // watermark onward is covered by the union, nothing lost or duplicated.
    assert_eq!(
      ack
        .replay
        .iter()
        .map(|e| e.payload["kind"].clone())
        .collect::<Vec<_>>(),
      vec!["before-2"]
    );
    let mut live = Vec::new();
    while let Ok(envelope) = receiver.recv_timeout(Duration::from_millis(50)) {
      live.push(envelope.payload["kind"].as_str().unwrap().to_string());
    }
    assert_eq!(live, ["after-3", "after-4"]);
    let mut union: Vec<String> = ack
      .replay
      .iter()
      .map(|e| e.payload["kind"].as_str().unwrap().to_string())
      .chain(live)
      .collect();
    union.sort();
    assert_eq!(union, ["after-3", "after-4", "before-2"]);
  }
}
