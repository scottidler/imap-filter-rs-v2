// src/imap_filter.rs

use eyre::{Result, eyre};
use imap::Session;
use log::{debug, info};
use native_tls::TlsStream;
use std::net::TcpStream;
use chrono::Utc;

use crate::cfg::config::Config;
use crate::cfg::message_filter::{MessageFilter, FilterAction};
use crate::cfg::state_filter::{StateFilter, StateAction};
use crate::message::Message;
use crate::utils::{get_labels, set_label, uid_move_gmail};

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

    /// High‐level entry point: fetch everything, then run both phases ACL-style.
    pub fn execute(&mut self) -> Result<()> {
        debug!("Entering IMAPFilter.execute");

        // 1) Fetch the inbox once
        info!("Fetching all messages from INBOX");
        let mut messages = self.fetch_messages()?;
        info!("✅ Fetched {} messages", messages.len());

        // 2) Phase 1 – MessageFilters
        info!("→ Phase 1: applying {} MessageFilters", self.message_filters.len());
        for message_filter in self.message_filters.clone() {
            info!(
                "Applying message filter '{}' to {} messages",
                message_filter.name,
                messages.len()
            );
            let mut remaining = Vec::with_capacity(messages.len());
            for msg in messages.drain(..) {
                debug!("Checking UID {} against filter '{}'", msg.uid, message_filter.name);
                if message_filter.matches(&msg) {
                    if let Some(action) = message_filter.actions.first() {
                        info!(
                            "Filter '{}' matched UID {}; applying action {:?}",
                            message_filter.name, msg.uid, action
                        );
                        self.apply_message_action(&msg, action)?;
                    }
                } else {
                    remaining.push(msg);
                }
            }
            messages = remaining;
            info!(
                "After '{}', {} messages remain",
                message_filter.name,
                messages.len()
            );
        }

        // 3) Phase 2 – StateFilters
        let now = Utc::now();
        info!("→ Phase 2: applying {} StateFilters", self.state_filters.len());
        for state_filter in self.state_filters.clone() {
            info!(
                "Applying state filter '{}' to {} messages",
                state_filter.name,
                messages.len()
            );
            let mut remaining = Vec::with_capacity(messages.len());
            for msg in messages.drain(..) {
                debug!("Checking TTL for UID {} under state '{}'", msg.uid, state_filter.name);
                if state_filter.matches(&msg) {
                    if let Some(action) = state_filter.evaluate_ttl(&msg, now)? {
                        if !state_filter.nerf {
                            info!(
                                "State '{}' expired for UID {}; applying {:?}",
                                state_filter.name, msg.uid, action
                            );
                            self.apply_state_action(&msg, &action)?;
                        } else {
                            info!("NERF [{}] would {:?}", state_filter.name, action);
                        }
                    } else {
                        debug!("State '{}' not yet expired for UID {}", state_filter.name, msg.uid);
                        remaining.push(msg);
                    }
                } else {
                    debug!(
                        "State '{}' does not match labels for UID {}",
                        state_filter.name, msg.uid
                    );
                    remaining.push(msg);
                }
            }
            messages = remaining;
            info!(
                "After state '{}', {} messages remain",
                state_filter.name,
                messages.len()
            );
        }

        debug!("Finished all filters; {} messages untouched", messages.len());
        info!("Logging out from IMAP");
        self.client.logout()?;
        info!("✅ IMAP Filter execution completed");
        Ok(())
    }

    /// Fetch UID, seq, FLAGS, X-GM-LABELS, INTERNALDATE, and the full header.
    fn fetch_messages(&mut self) -> Result<Vec<Message>> {
        debug!("Fetching all messages from INBOX");

        // 1) Select the mailbox
        self.client.select("INBOX")?;

        // 2) Search for *sequence numbers* of every message
        let seqs = self.client.search("ALL")?;
        debug!("SEARCH returned {} messages in INBOX", seqs.len());
        if seqs.is_empty() {
            return Ok(vec![]);
        }

        // 3) Build a comma-separated sequence-set
        let seq_set = seqs
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join(",");
        debug!("FETCHing headers for sequences: {}", seq_set);

        // 4) Fetch only the header fields we need, plus UID & INTERNALDATE
        let fetches = self.client.fetch(
            &seq_set,
            "(UID INTERNALDATE BODY[HEADER.FIELDS (TO CC FROM SUBJECT)])",
        )?;
        debug!("FETCH returned {} records", fetches.len());

        let mut out = Vec::with_capacity(fetches.len());
        for fetch in fetches.iter() {
            // a) UID and sequence number
            let uid = fetch.uid.unwrap_or(0);
            let seq = fetch.message;
            debug!("Parsing FETCH record: seq={}, uid={}", seq, uid);

            // b) Raw header bytes for the four fields
            let raw_header = fetch.body().unwrap_or(&[]).to_vec();

            // c) Internal date → RFC3339 string
            let date_str = fetch
                .internal_date()
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default();

            // d) Pull Gmail labels via our helper
            let label_set = get_labels(&mut self.client, uid)?;
            let raw_labels: Vec<String> = label_set.into_iter().collect();

            // e) Construct the Message
            let msg = Message::new(uid, seq, raw_header, raw_labels, date_str);
            debug!("Constructed Message struct for UID {}", uid);
            out.push(msg);
        }

        debug!("Successfully fetched {} messages", out.len());
        Ok(out)
    }

    fn apply_message_action(&mut self, msg: &Message, action: &FilterAction) -> Result<()> {
        match action {
            FilterAction::Star => {
                info!("⭐ Starring UID {}", msg.uid);
                set_label(&mut self.client, msg.uid, "\\Starred", &msg.subject)?;
            }
            FilterAction::Flag => {
                info!("🚩 Flagging UID {}", msg.uid);
                set_label(&mut self.client, msg.uid, "\\Important", &msg.subject)?;
            }
            FilterAction::Move(label) => {
                info!("➡️ Moving UID {} → {}", msg.uid, label);
                uid_move_gmail(&mut self.client, msg.uid, label, &msg.subject)?;
            }
        }
        Ok(())
    }

    fn apply_state_action(&mut self, msg: &Message, action: &StateAction) -> Result<()> {
        match action {
            StateAction::Delete => {
                info!("🗑 Deleting UID {}", msg.uid);
                self.client.uid_store(msg.uid.to_string(), "+FLAGS (\\Deleted)")?;
            }
            StateAction::Move(label) => {
                info!("➡️ Moving UID {} → {}", msg.uid, label);
                uid_move_gmail(&mut self.client, msg.uid, label, &msg.subject)?;
            }
        }
        Ok(())
    }
}
