use eyre::Result;
use imap::Session;
use log::debug;
use native_tls::TlsStream;
use std::collections::{HashMap, HashSet};
use std::net::TcpStream;

use crate::cfg::message_filter::FilterAction;
use crate::cfg::state_filter::{StateAction, StateFilter};
use crate::client_ops::{Clock, RealClock};
use crate::message::Message;

/// Builds a thread map from messages using available thread identification methods.
///
/// Priority order:
/// 1. Gmail X-GM-THRID (if available)
/// 2. Standard headers: Message-ID, In-Reply-To, References
///
/// For standard headers, we build a union-find structure to group related messages.
pub fn build_thread_map(messages: &[Message]) -> HashMap<String, Vec<Message>> {
    let mut thread_map: HashMap<String, Vec<Message>> = HashMap::new();

    // First pass: collect all messages with Gmail thread IDs
    let mut messages_without_gmail_thread: Vec<&Message> = Vec::new();

    for msg in messages {
        if let Some(thread_id) = &msg.thread_id {
            // Gmail thread ID available - use it directly
            thread_map.entry(thread_id.clone()).or_default().push(msg.clone());
        } else {
            messages_without_gmail_thread.push(msg);
        }
    }

    // If all messages have Gmail thread IDs, we're done
    if messages_without_gmail_thread.is_empty() {
        return thread_map;
    }

    // Second pass: build thread groups using standard headers
    // Build adjacency: which Message-IDs are related
    let mut related: HashMap<String, HashSet<String>> = HashMap::new();

    for msg in &messages_without_gmail_thread {
        let msg_id = msg.message_id.clone().unwrap_or_default();
        if msg_id.is_empty() {
            continue;
        }

        // In-Reply-To links this message to its parent
        if let Some(ref parent_id) = msg.in_reply_to {
            related.entry(msg_id.clone()).or_default().insert(parent_id.clone());
            related.entry(parent_id.clone()).or_default().insert(msg_id.clone());
        }

        // References links this message to all ancestors
        for ref_id in &msg.references {
            related.entry(msg_id.clone()).or_default().insert(ref_id.clone());
            related.entry(ref_id.clone()).or_default().insert(msg_id.clone());
        }
    }

    // Find connected components (thread groups) using BFS
    let mut visited: HashSet<String> = HashSet::new();
    let mut component_id = 0;

    for msg in &messages_without_gmail_thread {
        let msg_id = match &msg.message_id {
            Some(id) if !id.is_empty() => id.clone(),
            _ => continue,
        };

        if visited.contains(&msg_id) {
            continue;
        }

        // BFS to find all connected message IDs
        let mut component: HashSet<String> = HashSet::new();
        let mut queue = vec![msg_id.clone()];

        while let Some(current) = queue.pop() {
            if visited.contains(&current) {
                continue;
            }
            visited.insert(current.clone());
            component.insert(current.clone());

            if let Some(neighbors) = related.get(&current) {
                for neighbor in neighbors {
                    if !visited.contains(neighbor) {
                        queue.push(neighbor.clone());
                    }
                }
            }
        }

        // Create a thread ID for this component
        let thread_id = format!("std-thread-{}", component_id);
        component_id += 1;

        // Add all messages in this component to the thread map
        for msg in &messages_without_gmail_thread {
            if let Some(ref mid) = msg.message_id {
                if component.contains(mid) {
                    thread_map.entry(thread_id.clone()).or_default().push((*msg).clone());
                }
            }
        }
    }

    // Handle messages with no Message-ID (each is its own "thread")
    for msg in &messages_without_gmail_thread {
        if msg.message_id.is_none() || msg.message_id.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
            let solo_thread_id = format!("solo-uid-{}", msg.uid);
            thread_map.entry(solo_thread_id).or_default().push((*msg).clone());
        }
    }

    debug!(
        "Built thread map: {} threads from {} messages",
        thread_map.len(),
        messages.len()
    );

    thread_map
}

pub struct ThreadProcessor {
    thread_map: HashMap<String, Vec<Message>>,
}

impl ThreadProcessor {
    pub fn new(messages: &[Message]) -> Self {
        let thread_map = build_thread_map(messages);
        Self { thread_map }
    }

    /// Get the thread ID for a message, if it's part of a thread
    pub fn get_thread_id(&self, msg: &Message) -> Option<String> {
        // First check Gmail thread ID
        if let Some(tid) = &msg.thread_id {
            if self.thread_map.contains_key(tid) {
                return Some(tid.clone());
            }
        }

        // Check standard thread ID (computed from Message-ID)
        for (thread_id, msgs) in &self.thread_map {
            if msgs.iter().any(|m| m.uid == msg.uid) {
                return Some(thread_id.clone());
            }
        }

        None
    }

    /// Processes a message filter action across an entire thread
    pub fn process_thread_message_filter(
        &self,
        client: &mut Session<TlsStream<TcpStream>>,
        msg: &Message,
        action: &FilterAction,
    ) -> Result<Vec<Message>> {
        let mut processed = Vec::new();

        // Find the thread this message belongs to
        if let Some(thread_id) = self.get_thread_id(msg) {
            if let Some(thread_msgs) = self.thread_map.get(&thread_id) {
                debug!("Processing thread {} with {} messages", thread_id, thread_msgs.len());
                for thread_msg in thread_msgs {
                    // Apply the same action to each message in thread
                    crate::imap_filter::apply_message_action(client, thread_msg, action)?;
                    processed.push(thread_msg.clone());
                }
            }
        } else {
            // Not part of a thread, just process the single message
            crate::imap_filter::apply_message_action(client, msg, action)?;
            processed.push(msg.clone());
        }

        Ok(processed)
    }

    /// Processes a state filter action across an entire thread.
    /// TTL is evaluated based on the NEWEST message in the thread.
    /// The thread only expires when the newest message has exceeded TTL.
    pub fn process_thread_state_filter(
        &self,
        client: &mut Session<TlsStream<TcpStream>>,
        msg: &Message,
        filter: &StateFilter,
        action: &StateAction,
    ) -> Result<Vec<Message>> {
        self.process_thread_state_filter_with_clock(client, msg, filter, action, &RealClock)
    }

    /// Processes a state filter action across an entire thread with a custom clock.
    /// TTL is evaluated based on the NEWEST message in the thread.
    /// The thread only expires when the newest message has exceeded TTL.
    pub fn process_thread_state_filter_with_clock<C: Clock>(
        &self,
        client: &mut Session<TlsStream<TcpStream>>,
        msg: &Message,
        filter: &StateFilter,
        action: &StateAction,
        clock: &C,
    ) -> Result<Vec<Message>> {
        let mut processed = Vec::new();

        // Find the thread this message belongs to
        if let Some(thread_id) = self.get_thread_id(msg) {
            if let Some(thread_msgs) = self.thread_map.get(&thread_id) {
                // Find newest message in thread (by date)
                let newest_msg = thread_msgs.iter().max_by_key(|m| m.date.clone()).unwrap_or(msg);

                // Evaluate TTL based on the newest message only
                // If the newest message has expired, the whole thread expires
                let thread_expired = filter
                    .evaluate_ttl(newest_msg, clock)
                    .map(|opt| opt.is_some())
                    .unwrap_or(false);

                if thread_expired {
                    debug!(
                        "Thread {} expired (newest msg UID {} from {} dated {})",
                        thread_id,
                        newest_msg.uid,
                        newest_msg.sender_display(),
                        newest_msg.date
                    );
                    for thread_msg in thread_msgs {
                        crate::imap_filter::apply_state_action(client, thread_msg, action)?;
                        processed.push(thread_msg.clone());
                    }
                }
            }
        } else {
            // Not part of a thread, evaluate normally
            if let Ok(Some(_)) = filter.evaluate_ttl(msg, clock) {
                crate::imap_filter::apply_state_action(client, msg, action)?;
                processed.push(msg.clone());
            }
        }

        Ok(processed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfg::label::Label;

    fn make_message(
        uid: u32,
        thread_id: Option<&str>,
        message_id: Option<&str>,
        in_reply_to: Option<&str>,
        references: Vec<&str>,
    ) -> Message {
        Message {
            uid,
            seq: uid,
            to: vec![],
            cc: vec![],
            from: vec![],
            subject: format!("Test message {}", uid),
            date: "2024-01-15T10:00:00+00:00".to_string(),
            labels: vec![Label::Inbox],
            headers: std::collections::HashMap::new(),
            message_id: message_id.map(String::from),
            in_reply_to: in_reply_to.map(String::from),
            references: references.into_iter().map(String::from).collect(),
            thread_id: thread_id.map(String::from),
        }
    }

    #[test]
    fn test_build_thread_map_gmail_thread_ids() {
        let messages = vec![
            make_message(1, Some("gmail-thread-1"), None, None, vec![]),
            make_message(2, Some("gmail-thread-1"), None, None, vec![]),
            make_message(3, Some("gmail-thread-2"), None, None, vec![]),
        ];

        let thread_map = build_thread_map(&messages);

        assert_eq!(thread_map.len(), 2);
        assert_eq!(thread_map.get("gmail-thread-1").unwrap().len(), 2);
        assert_eq!(thread_map.get("gmail-thread-2").unwrap().len(), 1);
    }

    #[test]
    fn test_build_thread_map_standard_headers() {
        // Simulate a thread:
        // msg1: root message
        // msg2: reply to msg1
        // msg3: reply to msg2 (references both msg1 and msg2)
        let messages = vec![
            make_message(1, None, Some("<msg1@test.com>"), None, vec![]),
            make_message(2, None, Some("<msg2@test.com>"), Some("<msg1@test.com>"), vec![]),
            make_message(
                3,
                None,
                Some("<msg3@test.com>"),
                Some("<msg2@test.com>"),
                vec!["<msg1@test.com>", "<msg2@test.com>"],
            ),
        ];

        let thread_map = build_thread_map(&messages);

        // All three messages should be in the same thread
        assert_eq!(thread_map.len(), 1);
        let thread = thread_map.values().next().unwrap();
        assert_eq!(thread.len(), 3);
    }

    #[test]
    fn test_build_thread_map_separate_threads() {
        // Two separate conversations
        let messages = vec![
            make_message(1, None, Some("<thread1-msg1@test.com>"), None, vec![]),
            make_message(
                2,
                None,
                Some("<thread1-msg2@test.com>"),
                Some("<thread1-msg1@test.com>"),
                vec![],
            ),
            make_message(3, None, Some("<thread2-msg1@test.com>"), None, vec![]),
            make_message(
                4,
                None,
                Some("<thread2-msg2@test.com>"),
                Some("<thread2-msg1@test.com>"),
                vec![],
            ),
        ];

        let thread_map = build_thread_map(&messages);

        // Two separate threads
        assert_eq!(thread_map.len(), 2);
    }

    #[test]
    fn test_build_thread_map_no_message_id() {
        // Messages without Message-ID become solo threads
        let messages = vec![
            make_message(1, None, None, None, vec![]),
            make_message(2, None, None, None, vec![]),
        ];

        let thread_map = build_thread_map(&messages);

        // Each message is its own "thread"
        assert_eq!(thread_map.len(), 2);
    }

    #[test]
    fn test_build_thread_map_mixed_gmail_and_standard() {
        let messages = vec![
            // Gmail thread
            make_message(1, Some("gmail-thread-1"), None, None, vec![]),
            make_message(2, Some("gmail-thread-1"), None, None, vec![]),
            // Standard thread
            make_message(3, None, Some("<std-msg1@test.com>"), None, vec![]),
            make_message(
                4,
                None,
                Some("<std-msg2@test.com>"),
                Some("<std-msg1@test.com>"),
                vec![],
            ),
        ];

        let thread_map = build_thread_map(&messages);

        // Should have 2 threads: 1 Gmail + 1 standard
        assert_eq!(thread_map.len(), 2);
        assert!(thread_map.contains_key("gmail-thread-1"));
    }

    #[test]
    fn test_thread_processor_get_thread_id() {
        let messages = vec![
            make_message(1, Some("gmail-thread-1"), None, None, vec![]),
            make_message(2, None, Some("<msg@test.com>"), None, vec![]),
        ];

        let processor = ThreadProcessor::new(&messages);

        // Gmail thread ID should be found
        assert_eq!(
            processor.get_thread_id(&messages[0]),
            Some("gmail-thread-1".to_string())
        );

        // Standard thread ID should be found
        let thread_id = processor.get_thread_id(&messages[1]);
        assert!(thread_id.is_some());
        assert!(thread_id.unwrap().starts_with("std-thread-"));
    }
}
