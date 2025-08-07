// src/imap_filter.rs

use eyre::{Result, eyre};
use imap::Session;
use log::{debug, error, info};
use native_tls::TlsStream;
use std::net::TcpStream;
use chrono::Utc;
use std::collections::HashMap;

use crate::cfg::config::Config;
use crate::cfg::message_filter::{MessageFilter, FilterAction};
use crate::cfg::state_filter::{StateFilter, StateAction, TTL};
use crate::utils::{get_labels, set_label, uid_move_gmail};
use crate::message::Message;
use crate::thread::ThreadProcessor;

pub fn apply_message_action(
    client: &mut Session<TlsStream<TcpStream>>,
    msg: &Message,
    action: &FilterAction,
) -> Result<()> {
    match action {
        FilterAction::Star => {
            info!("⭐ Starring UID {}", msg.uid);
            set_label(client, msg.uid, "\\Starred", &msg.subject)?;
        }
        FilterAction::Flag => {
            info!("🚩 Flagging UID {}", msg.uid);
            set_label(client, msg.uid, "\\Important", &msg.subject)?;
        }
        FilterAction::Move(label) => {
            info!("➡️ Moving UID {} → {}", msg.uid, label);
            uid_move_gmail(client, msg.uid, label, &msg.subject)?;
        }
    }
    Ok(())
}

pub fn apply_state_action(
    client: &mut Session<TlsStream<TcpStream>>,
    msg: &Message,
    action: &StateAction,
) -> Result<()> {
    match action {
        StateAction::Delete => {
            info!("🗑 Deleting UID {}", msg.uid);
            client.uid_store(msg.uid.to_string(), "+FLAGS (\\Deleted)")?;
        }
        StateAction::Move(label) => {
            info!("➡️ Moving UID {} → {}", msg.uid, label);
            uid_move_gmail(client, msg.uid, label, &msg.subject)?;
        }
    }
    Ok(())
}

pub struct IMAPFilter {
    pub client: Session<TlsStream<TcpStream>>,
    pub message_filters: Vec<MessageFilter>,
    pub state_filters: Vec<StateFilter>,
}

impl IMAPFilter {
    pub fn new(
        client: Session<TlsStream<TcpStream>>,
        config: Config,
    ) -> Self {
        debug!(
            "Initializing IMAPFilter with {} message_filters and {} state_filters",
            config.message_filters.len(),
            config.state_filters.len(),
        );

        IMAPFilter {
            client,
            message_filters: config.message_filters,
            state_filters: config.state_filters,
        }
    }

    fn fetch_messages(&mut self) -> Result<Vec<Message>> {
        debug!("Fetching all messages from INBOX");

        // 1) Select mailbox
        self.client.select("INBOX")?;

        // 2) Search all messages
        let seqs = self.client.search("ALL")?;
        debug!("SEARCH returned {} messages in INBOX", seqs.len());
        if seqs.is_empty() {
            return Ok(vec![]);
        }

        // 3) Build sequence-set
        let seq_set = seqs.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(",");
        debug!("FETCHing records for sequences: {}", seq_set);

        // 4) Fetch UID, FLAGS, INTERNALDATE, thread info and full header
        let fetches = self.client.fetch(
            &seq_set, 
            "(UID FLAGS INTERNALDATE X-GM-THRID RFC822.HEADER)"
        )?;
        debug!("FETCH returned {} records", fetches.len());

        // 5) Build a map of thread IDs to messages for later processing
        let mut thread_map: HashMap<String, Vec<Message>> = HashMap::new();

        let mut out = Vec::with_capacity(fetches.len());
        for fetch in fetches.iter() {
            let uid = fetch.uid.unwrap_or(0);
            let seq = fetch.message;
            debug!("Parsing FETCH record: seq={}, uid={}", seq, uid);

            // extract full header bytes
            let raw_header = fetch.header().unwrap_or(&[]).to_vec();
            // DEBUG: dump raw headers for diagnostics
            let header_text = String::from_utf8_lossy(&raw_header).into_owned();
            //debug!("Raw header for UID {}:\n{}", uid, header_text);

            // convert internal date
            let date_str = fetch.internal_date().map(|dt| dt.to_rfc3339()).unwrap_or_default();

            // labels
            let mut label_set = get_labels(&mut self.client, uid)?;
            for flag in fetch.flags() {
                label_set.insert(flag.to_string());
            }
            let raw_labels: Vec<String> = label_set.into_iter().collect();

            // build Message
            let msg = Message::new(uid, seq, raw_header, raw_labels, date_str);

            if msg.from.is_empty() && msg.to.is_empty() && msg.cc.is_empty() {
                error!("UID {} address fields empty. Header was:\n{}", uid, header_text);
            }
            assert!(!msg.from.is_empty() || !msg.to.is_empty() || !msg.cc.is_empty(),
                "No address fields (To/Cc/From) for UID {}", uid
            );

            // Group messages by thread ID if available
            if let Some(thread_id) = &msg.thread_id {
                thread_map.entry(thread_id.clone())
                    .or_default()
                    .push(msg.clone());
            }

            out.push(msg);
        }

        debug!("Successfully fetched {} messages", out.len());
        Ok(out)
    }

    pub fn execute(&mut self) -> Result<()> {
        debug!("Entering IMAPFilter.execute");

        info!("Fetching all messages from INBOX");
        let mut messages = self.fetch_messages()?;
        info!("✅ Fetched {} messages", messages.len());
        for message in &messages {
            debug!("message: {:#?}", message);
        }

        // Create thread map from messages
        let thread_map: HashMap<String, Vec<Message>> = messages.iter()
            .filter_map(|msg| {
                msg.thread_id.as_ref().map(|tid| (tid.clone(), msg.clone()))
            })
            .fold(HashMap::new(), |mut map, (tid, msg)| {
                map.entry(tid).or_default().push(msg);
                map
            });

        let thread_processor = ThreadProcessor::new(thread_map);
        self.process_message_filters_with_threads(&mut messages, &thread_processor)?;
        self.process_state_filters_with_threads(&mut messages, &thread_processor)?;

        debug!("Finished all filters; {} messages untouched", messages.len());
        info!("Logging out from IMAP");
        self.client.logout()?;
        info!("✅ IMAP Filter execution completed");
        Ok(())
    }

    fn process_message_filters_with_threads(
        &mut self,
        messages: &mut Vec<Message>,
        thread_processor: &ThreadProcessor,
    ) -> Result<()> {
        info!("→ Phase 1: applying {} MessageFilters", self.message_filters.len());

        let mut i = 0;
        while i < messages.len() {
            let msg = &messages[i];

            let matched = self.message_filters.iter().find_map(|message_filter| {
                if message_filter.matches(msg) {
                    message_filter.actions.first().map(|action| (message_filter.name.clone(), action.clone()))
                } else {
                    None
                }
            });

            if let Some((filter_name, action)) = matched {
                info!(
                    "Filter '{}' matched UID {}; applying action {:?}",
                    filter_name, msg.uid, action
                );
                
                // Process entire thread
                let processed = thread_processor.process_thread_message_filter(
                    &mut self.client,
                    msg,
                    &self.message_filters[i],
                    &action
                )?;

                // Remove all processed messages from the list
                messages.retain(|m| !processed.iter().any(|p| p.uid == m.uid));
            } else {
                i += 1;
            }
        }

        Ok(())
    }

    fn process_state_filters_with_threads(
        &mut self,
        messages: &mut Vec<Message>,
        thread_processor: &ThreadProcessor,
    ) -> Result<()> {
        let now = Utc::now();
        info!("→ Phase 2: applying {} StateFilters", self.state_filters.len());

        let mut i = 0;
        while i < messages.len() {
            let msg = &messages[i];

            if let Some(state_filter) = self.state_filters.iter().find(|sf| sf.matches(msg)) {
                if let TTL::Keep = state_filter.ttl {
                    debug!(
                        "State '{}' is Keep; protecting UID {} from further filters",
                        state_filter.name, msg.uid
                    );
                    messages.remove(i);
                    continue;
                }

                // Process entire thread for TTL
                let processed = thread_processor.process_thread_state_filter(
                    &mut self.client,
                    msg,
                    state_filter,
                    &state_filter.action
                )?;

                // Remove all processed messages from the list
                messages.retain(|m| !processed.iter().any(|p| p.uid == m.uid));
            } else {
                debug!(
                    "No state filter matched UID {}; retaining for next filter",
                    msg.uid
                );
                i += 1;
            }
        }

        Ok(())
    }
}
