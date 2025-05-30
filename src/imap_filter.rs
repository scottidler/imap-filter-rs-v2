// src/imap_filter.rs

use eyre::{Result, eyre};
use imap::Session;
use log::{debug, info};
use native_tls::TlsStream;
use std::net::TcpStream;
use chrono::Utc;

use crate::cfg::config::Config;
use crate::cfg::message_filter::{MessageFilter, FilterAction};
use crate::cfg::state_filter::{StateFilter, StateAction, TTL};
use crate::utils::{get_labels, set_label, uid_move_gmail};
use crate::message::Message;

fn apply_message_action(
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

fn apply_state_action(
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

    /// Fetch UID, seq, FLAGS, INTERNALDATE, and the full RFC-2822 header.
    fn fetch_messages(&mut self) -> Result<Vec<Message>> {
        debug!("Fetching all messages from INBOX");

        // 1) Select the mailbox
        self.client.select("INBOX")?;

        // 2) Search for all message sequence numbers
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
        debug!("FETCHing records for sequences: {}", seq_set);

        // 4) Fetch UID, FLAGS, INTERNALDATE, and full message header
        let fetches = self.client.fetch(&seq_set, "(UID FLAGS INTERNALDATE RFC822.HEADER)")?;
        debug!("FETCH returned {} records", fetches.len());

        let mut out = Vec::with_capacity(fetches.len());
        for fetch in fetches.iter() {
            // a) UID and sequence number
            let uid = fetch.uid.unwrap_or(0);
            let seq = fetch.message;
            debug!("Parsing FETCH record: seq={}, uid={}", seq, uid);

            // b) Full RFC-2822 header bytes
            let raw_header = fetch.header().unwrap_or(&[]).to_vec();
            assert!(
                !raw_header.is_empty(),
                "Empty fetched header for UID {}",
                uid
            );

            // c) Internal date → RFC3339 string
            let date_str = fetch
                .internal_date()
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default();

            // d) Retrieve Gmail labels & merge FLAGS
            let mut label_set = get_labels(&mut self.client, uid)?;
            for flag in fetch.flags() {
                label_set.insert(flag.to_string());
            }
            let raw_labels: Vec<String> = label_set.into_iter().collect();

            // e) Construct Message
            let msg = Message::new(uid, seq, raw_header, raw_labels, date_str);

            // f) Sanity-checks on parsed fields
            assert!(
                !msg.headers.is_empty(),
                "Missing headers map for UID {}",
                uid
            );
            assert!(
                !msg.subject.is_empty(),
                "Missing subject header for UID {}",
                uid
            );
            assert!(
                !msg.from.is_empty() || !msg.to.is_empty() || !msg.cc.is_empty(),
                "No address fields (To/Cc/From) for UID {}",
                uid
            );

            debug!("Constructed Message struct for UID {}", uid);
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

        self.process_message_filters(&mut messages)?;
        self.process_state_filters(&mut messages)?;

        debug!("Finished all filters; {} messages untouched", messages.len());
        info!("Logging out from IMAP");
        self.client.logout()?;
        info!("✅ IMAP Filter execution completed");
        Ok(())
    }

    fn process_message_filters(&mut self, messages: &mut Vec<Message>) -> Result<()> {
        info!("→ Phase 1: applying {} MessageFilters", self.message_filters.len());

        let mut i = 0;
        while i < messages.len() {
            let msg = &messages[i];
            //debug!("message: {:#?}", msg);

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
                apply_message_action(&mut self.client, msg, &action)?;
                messages.remove(i);
            } else {
                i += 1;
            }
        }

        Ok(())
    }

    fn process_state_filters(&mut self, messages: &mut Vec<Message>) -> Result<()> {
        let now = Utc::now();
        info!("→ Phase 2: applying {} StateFilters", self.state_filters.len());

        let mut i = 0;
        while i < messages.len() {
            let msg = &messages[i];
            //debug!("message: {:#?}", msg);

            if let Some(state_filter) = self.state_filters.iter().find(|sf| sf.matches(msg)) {
                if let TTL::Keep = state_filter.ttl {
                    debug!(
                        "State '{}' is Keep; protecting UID {} from further filters",
                        state_filter.name, msg.uid
                    );
                    messages.remove(i);
                    continue;
                }

                if let Some(action) = state_filter.evaluate_ttl(msg, now)? {
                    if !state_filter.nerf {
                        info!(
                            "State '{}' expired for UID {}; applying {:?}",
                            state_filter.name, msg.uid, action
                        );
                        apply_state_action(&mut self.client, msg, &action)?;
                    } else {
                        info!("NERF [{}] would {:?}", state_filter.name, action);
                    }
                    messages.remove(i);
                } else {
                    debug!(
                        "State '{}' not yet expired for UID {}",
                        state_filter.name, msg.uid
                    );
                    i += 1;
                }
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
