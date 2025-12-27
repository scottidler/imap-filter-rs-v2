use eyre::Result;
use imap::Session;
use log::debug;
use native_tls::TlsStream;
use std::collections::HashMap;
use std::net::TcpStream;

use crate::cfg::message_filter::FilterAction;
use crate::cfg::state_filter::{StateAction, StateFilter};
use crate::message::Message;

pub struct ThreadProcessor {
    thread_map: HashMap<String, Vec<Message>>,
}

impl ThreadProcessor {
    pub fn new(thread_map: HashMap<String, Vec<Message>>) -> Self {
        Self { thread_map }
    }

    /// Processes a message filter action across an entire thread
    pub fn process_thread_message_filter(
        &self,
        client: &mut Session<TlsStream<TcpStream>>,
        msg: &Message,
        action: &FilterAction,
    ) -> Result<Vec<Message>> {
        let mut processed = Vec::new();

        // If message is part of a thread, apply action to all messages in thread
        if let Some(thread_id) = &msg.thread_id {
            if let Some(thread_msgs) = self.thread_map.get(thread_id) {
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
        let mut processed = Vec::new();
        let now = chrono::Utc::now();

        // If message is part of a thread, evaluate TTL based on newest message
        if let Some(thread_id) = &msg.thread_id {
            if let Some(thread_msgs) = self.thread_map.get(thread_id) {
                // Find newest message in thread (by date)
                let newest_msg = thread_msgs.iter().max_by_key(|m| m.date.clone()).unwrap_or(msg);

                // Evaluate TTL based on the newest message only
                // If the newest message has expired, the whole thread expires
                let thread_expired = filter
                    .evaluate_ttl(newest_msg, now)
                    .map(|opt| opt.is_some())
                    .unwrap_or(false);

                if thread_expired {
                    debug!(
                        "Thread {} expired (newest msg UID {} date {})",
                        thread_id, newest_msg.uid, newest_msg.date
                    );
                    for thread_msg in thread_msgs {
                        crate::imap_filter::apply_state_action(client, thread_msg, action)?;
                        processed.push(thread_msg.clone());
                    }
                }
            }
        } else {
            // Not part of a thread, evaluate normally
            if let Ok(Some(_)) = filter.evaluate_ttl(msg, now) {
                crate::imap_filter::apply_state_action(client, msg, action)?;
                processed.push(msg.clone());
            }
        }

        Ok(processed)
    }
}
