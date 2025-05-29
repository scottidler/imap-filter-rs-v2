// src/imap_filter.rs

use eyre::{Result, eyre};
use imap::Session;
use log::{debug, info};
use native_tls::TlsStream;
use std::net::TcpStream;
use chrono::Utc;

use crate::cfg::message_filter::{MessageFilter, FilterAction};
use crate::cfg::state_filter::{StateFilter, StateAction};
use crate::message::Message;
use crate::utils::{get_labels, uid_move_gmail};

pub struct IMAPFilter {
    pub client: Session<TlsStream<TcpStream>>,
    pub message_filters: Vec<MessageFilter>,
    pub state_filters: Vec<StateFilter>,
}

impl IMAPFilter {
    pub fn new(
        client: Session<TlsStream<TcpStream>>,
        message_filters: Vec<MessageFilter>,
        state_filters: Vec<StateFilter>,
    ) -> Self {
        IMAPFilter { client, message_filters, state_filters }
    }

    /// High‐level entry point: fetch everything, then run both phases ACL-style.
    pub fn execute(&mut self) -> Result<()> {
        debug!("Starting IMAPFilter.execute");

        // 1) Fetch the inbox once
        let mut messages = self.fetch_messages()?;
        debug!("Fetched {} messages", messages.len());

        // 2) Phase 1 – MessageFilters
        // clone out the filters so we don't hold an immutable borrow on `self`
        for message_filter in self.message_filters.clone() {
            info!("Applying message filter '{}'", message_filter.name);
            let mut remaining = Vec::with_capacity(messages.len());
            for msg in messages.drain(..) {
                if message_filter.matches(&msg) {
                    if let Some(action) = message_filter.actions.first() {
                        self.apply_message_action(&msg, action)?;
                    }
                } else {
                    remaining.push(msg);
                }
            }
            messages = remaining;
        }

        // 3) Phase 2 – StateFilters
        let now = Utc::now();
        for state_filter in self.state_filters.clone() {
            info!("Applying state filter '{}'", state_filter.name);
            let mut remaining = Vec::with_capacity(messages.len());
            for msg in messages.drain(..) {
                if state_filter.matches(&msg) {
                    if let Some(action) = state_filter.evaluate_ttl(&msg, now)? {
                        if !state_filter.nerf {
                            self.apply_state_action(&msg, &action)?;
                        } else {
                            info!("NERF [{}] would {:?}", state_filter.name, action);
                        }
                    }
                } else {
                    remaining.push(msg);
                }
            }
            messages = remaining;
        }

        debug!("Done; {} messages left untouched", messages.len());
        self.client.logout()?;
        Ok(())
    }

    /// Fetch UID, seq, FLAGS, X-GM-LABELS, INTERNALDATE, and the header fields.
    fn fetch_messages(&mut self) -> Result<Vec<Message>> {
        debug!("Selecting INBOX");
        self.client.select("INBOX")?;

        let uids = self.client.search("ALL")?;
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let seq_set = uids.iter().map(|u| u.to_string()).collect::<Vec<_>>().join(",");

        let fetches = self.client.uid_fetch(
            &seq_set,
            "(UID FLAGS X-GM-LABELS INTERNALDATE \
             BODY.PEEK[HEADER.FIELDS (TO CC FROM SUBJECT)])",
        )?;

        let mut out = Vec::with_capacity(fetches.len());
        for fetch in fetches.iter() {
            let uid = fetch.uid.ok_or_else(|| eyre!("Missing UID in FETCH"))?;
            let seq = fetch.message;
            let date_str = fetch
                .internal_date()
                .ok_or_else(|| eyre!("Missing INTERNALDATE"))?
                .to_rfc3339();

            // pull labels via our helper (instead of fetch.labels())
            let label_set = get_labels(&mut self.client, uid)?;
            let raw_labels: Vec<String> = label_set.into_iter().collect();

            let raw_headers = fetch
                .body()
                .ok_or_else(|| eyre!("Missing HEADER in FETCH"))?
                .to_vec();

            out.push(Message::new(uid, seq, raw_headers, raw_labels, date_str));
        }
        Ok(out)
    }

    fn apply_message_action(&mut self, msg: &Message, action: &FilterAction) -> Result<()> {
        match action {
            FilterAction::Star => {
                info!("⭐ Starring UID {}", msg.uid);
                self.client.uid_store(msg.uid.to_string(), "+X-GM-LABELS (\\Starred)")?;
            }
            FilterAction::Flag => {
                info!("🚩 Flagging UID {}", msg.uid);
                self.client.uid_store(msg.uid.to_string(), "+X-GM-LABELS (\\Important)")?;
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
