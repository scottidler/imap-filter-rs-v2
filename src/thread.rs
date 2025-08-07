use std::collections::HashMap;
use eyre::Result;
use imap::Session;
use native_tls::TlsStream;
use std::net::TcpStream;
use log::debug;

use crate::message::Message;
use crate::cfg::message_filter::{MessageFilter, FilterAction};
use crate::cfg::state_filter::{StateFilter, StateAction};

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
        filter: &MessageFilter,
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

    /// Processes a state filter action across an entire thread
    pub fn process_thread_state_filter(
        &self,
        client: &mut Session<TlsStream<TcpStream>>,
        msg: &Message,
        filter: &StateFilter,
        action: &StateAction,
    ) -> Result<Vec<Message>> {
        let mut processed = Vec::new();

        // If message is part of a thread, evaluate TTL based on newest message
        if let Some(thread_id) = &msg.thread_id {
            if let Some(thread_msgs) = self.thread_map.get(thread_id) {
                // Find newest message in thread
                let newest_msg = thread_msgs.iter()
                    .max_by_key(|m| m.date.clone())
                    .unwrap_or(msg);

                // Only expire if ALL messages in thread have passed TTL
                let all_expired = thread_msgs.iter().all(|m| {
                    filter.evaluate_ttl(m, chrono::Utc::now())
                        .map(|opt| opt.is_some())
                        .unwrap_or(false)
                });

                if all_expired {
                    for thread_msg in thread_msgs {
                        crate::imap_filter::apply_state_action(client, thread_msg, action)?;
                        processed.push(thread_msg.clone());
                    }
                }
            }
        } else {
            // Not part of a thread, evaluate normally
            if let Ok(Some(_)) = filter.evaluate_ttl(msg, chrono::Utc::now()) {
                crate::imap_filter::apply_state_action(client, msg, action)?;
                processed.push(msg.clone());
            }
        }

        Ok(processed)
    }
}