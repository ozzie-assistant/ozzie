use std::sync::{Arc, Mutex, RwLock};

use tokio::sync::broadcast;

use super::types::Event;

/// Interface for publishing and subscribing to events.
#[async_trait::async_trait]
pub trait EventBus: Send + Sync {
    fn publish(&self, event: Event);
    fn subscribe(&self, event_types: &[&str]) -> broadcast::Receiver<Event>;
    fn history(&self, limit: usize) -> Vec<Event>;
    fn history_filtered(&self, limit: usize, filter_type: &str) -> Vec<Event>;
}

/// In-memory event bus using `tokio::broadcast`.
pub struct Bus {
    sender: broadcast::Sender<Event>,
    ring_buffer: Arc<RwLock<RingBuffer>>,
    /// Filtered subscribers: (event_types, sender).
    filtered_subs: Mutex<Vec<(Vec<String>, broadcast::Sender<Event>)>>,
}

impl Bus {
    /// Creates a new event bus with the given buffer capacity.
    pub fn new(buffer_size: usize) -> Self {
        let (sender, _) = broadcast::channel(buffer_size);
        Self {
            sender,
            ring_buffer: Arc::new(RwLock::new(RingBuffer::new(buffer_size))),
            filtered_subs: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl EventBus for Bus {
    fn publish(&self, event: Event) {
        // Store in ring buffer
        if let Ok(mut rb) = self.ring_buffer.write() {
            rb.add(event.clone());
        }

        // Broadcast to all receivers (ignore error = no receivers)
        let _ = self.sender.send(event.clone());

        // Notify filtered subscribers
        if let Ok(subs) = self.filtered_subs.lock() {
            let event_type = event.event_type();
            for (types, sender) in subs.iter() {
                if types.is_empty() || types.iter().any(|t| t == event_type) {
                    let _ = sender.send(event.clone());
                }
            }
        }
    }

    fn subscribe(&self, event_types: &[&str]) -> broadcast::Receiver<Event> {
        if event_types.is_empty() {
            // Subscribe to all events
            return self.sender.subscribe();
        }

        // Create a filtered channel
        let (tx, rx) = broadcast::channel(256);
        if let Ok(mut subs) = self.filtered_subs.lock() {
            subs.push((event_types.iter().map(|s| s.to_string()).collect(), tx));
        }
        rx
    }

    fn history(&self, limit: usize) -> Vec<Event> {
        let rb = self.ring_buffer.read().unwrap();
        rb.get(limit)
    }

    fn history_filtered(&self, limit: usize, filter_type: &str) -> Vec<Event> {
        let rb = self.ring_buffer.read().unwrap();
        rb.get_filtered(limit, filter_type)
    }
}

/// Circular buffer for storing recent events.
struct RingBuffer {
    events: Vec<Option<Event>>,
    size: usize,
    pos: usize,
    count: usize,
}

impl RingBuffer {
    fn new(size: usize) -> Self {
        Self {
            events: (0..size).map(|_| None).collect(),
            size,
            pos: 0,
            count: 0,
        }
    }

    fn add(&mut self, event: Event) {
        self.events[self.pos] = Some(event);
        self.pos = (self.pos + 1) % self.size;
        if self.count < self.size {
            self.count += 1;
        }
    }

    fn get(&self, n: usize) -> Vec<Event> {
        let n = n.min(self.count);
        if n == 0 {
            return Vec::new();
        }
        let mut result = Vec::with_capacity(n);
        let start = (self.pos + self.size - n) % self.size;
        for i in 0..n {
            let idx = (start + i) % self.size;
            if let Some(event) = &self.events[idx] {
                result.push(event.clone());
            }
        }
        result
    }

    fn get_filtered(&self, n: usize, filter_type: &str) -> Vec<Event> {
        let mut result = Vec::new();
        for i in 0..self.count {
            if result.len() >= n {
                break;
            }
            let idx = (self.pos + self.size - 1 - i) % self.size;
            if let Some(event) = &self.events[idx]
                && event.event_type() == filter_type
            {
                result.push(event.clone());
            }
        }
        // Reverse to chronological order
        result.reverse();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventKind, EventPayload, EventSource};

    fn make_event(payload: EventPayload) -> Event {
        Event::new(EventSource::Agent, payload)
    }

    #[test]
    fn ring_buffer_basic() {
        let mut rb = RingBuffer::new(4);
        rb.add(make_event(EventPayload::user_message("hi")));
        rb.add(make_event(EventPayload::AssistantMessage {
            content: "hello".to_string(),
            error: None,
        }));
        rb.add(make_event(EventPayload::ToolCall {
            call_id: "tc_1".to_string(),
            tool: "t".to_string(),
            arguments: "{}".to_string(),
        }));

        let events = rb.get(10);
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn ring_buffer_wraps() {
        let mut rb = RingBuffer::new(3);
        for _ in 0..5 {
            rb.add(make_event(EventPayload::user_message("hi")));
        }
        let events = rb.get(10);
        assert_eq!(events.len(), 3); // only keeps 3
    }

    #[test]
    fn ring_buffer_filtered() {
        let mut rb = RingBuffer::new(10);
        rb.add(make_event(EventPayload::user_message("hi")));
        rb.add(make_event(EventPayload::AssistantMessage {
            content: "hello".to_string(),
            error: None,
        }));
        rb.add(make_event(EventPayload::user_message("bye")));
        rb.add(make_event(EventPayload::ToolCall {
            call_id: "tc_1".to_string(),
            tool: "t".to_string(),
            arguments: "{}".to_string(),
        }));

        let filtered = rb.get_filtered(10, "user.message");
        assert_eq!(filtered.len(), 2);
    }

    #[tokio::test]
    async fn bus_publish_subscribe() {
        let bus = Bus::new(64);
        let mut rx = bus.subscribe(&[EventKind::UserMessage.as_str()]);

        bus.publish(make_event(EventPayload::user_message("hi")));
        bus.publish(make_event(EventPayload::AssistantMessage {
            content: "hello".to_string(),
            error: None,
        })); // should be filtered

        let event = rx.recv().await.unwrap();
        assert_eq!(event.event_type(), "user.message");
    }

    #[tokio::test]
    async fn bus_subscribe_all() {
        let bus = Bus::new(64);
        let mut rx = bus.subscribe(&[]);

        bus.publish(make_event(EventPayload::user_message("hi")));
        bus.publish(make_event(EventPayload::AssistantMessage {
            content: "hello".to_string(),
            error: None,
        }));

        let e1 = rx.recv().await.unwrap();
        let e2 = rx.recv().await.unwrap();
        assert_eq!(e1.event_type(), "user.message");
        assert_eq!(e2.event_type(), "assistant.message");
    }

    #[test]
    fn bus_history() {
        let bus = Bus::new(64);
        bus.publish(make_event(EventPayload::user_message("hi")));
        bus.publish(make_event(EventPayload::AssistantMessage {
            content: "hello".to_string(),
            error: None,
        }));
        bus.publish(make_event(EventPayload::ToolCall {
            call_id: "tc_1".to_string(),
            tool: "t".to_string(),
            arguments: "{}".to_string(),
        }));

        let history = bus.history(2);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].event_type(), "assistant.message");
        assert_eq!(history[1].event_type(), "tool.call");
    }

    #[test]
    fn bus_history_filtered() {
        let bus = Bus::new(64);
        bus.publish(make_event(EventPayload::user_message("hi")));
        bus.publish(make_event(EventPayload::AssistantMessage {
            content: "hello".to_string(),
            error: None,
        }));
        bus.publish(make_event(EventPayload::user_message("bye")));

        let filtered = bus.history_filtered(10, "user.message");
        assert_eq!(filtered.len(), 2);
    }
}
