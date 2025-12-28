// src/imap_filter.rs

use eyre::Result;
use imap::{ImapConnection, Session};
use log::{debug, error, info};

use crate::cfg::config::Config;
use crate::cfg::message_filter::{FilterAction, MessageFilter};
use crate::cfg::state_filter::{StateAction, StateFilter, Ttl};
use crate::message::Message;
use crate::thread::ThreadProcessor;
use crate::utils::{set_label, uid_move_gmail};

pub fn apply_message_action<C: ImapConnection>(
    client: &mut Session<C>,
    msg: &Message,
    action: &FilterAction,
) -> Result<()> {
    let sender = msg.sender_display();
    match action {
        FilterAction::Star => {
            info!("⭐ Starring UID {} from {} - {}", msg.uid, sender, msg.subject);
            set_label(client, msg.uid, "\\Starred", &msg.subject)?;
        }
        FilterAction::Flag => {
            info!("🚩 Flagging UID {} from {} - {}", msg.uid, sender, msg.subject);
            set_label(client, msg.uid, "\\Important", &msg.subject)?;
        }
        FilterAction::Move(label) => {
            info!(
                "➡️ Moving UID {} from {} → {} - {}",
                msg.uid, sender, label, msg.subject
            );
            uid_move_gmail(client, msg.uid, label, &msg.subject)?;
        }
    }
    Ok(())
}

pub fn apply_state_action<C: ImapConnection>(
    client: &mut Session<C>,
    msg: &Message,
    action: &StateAction,
) -> Result<()> {
    let sender = msg.sender_display();
    match action {
        StateAction::Delete => {
            info!("🗑 Deleting UID {} from {} - {}", msg.uid, sender, msg.subject);
            client.uid_store(msg.uid.to_string(), "+FLAGS (\\Deleted)")?;
        }
        StateAction::Move(label) => {
            info!(
                "➡️ Moving UID {} from {} → {} - {}",
                msg.uid, sender, label, msg.subject
            );
            uid_move_gmail(client, msg.uid, label, &msg.subject)?;
        }
    }
    Ok(())
}

pub struct IMAPFilter<C: ImapConnection> {
    pub client: Session<C>,
    pub message_filters: Vec<MessageFilter>,
    pub state_filters: Vec<StateFilter>,
}

impl<C: ImapConnection> IMAPFilter<C> {
    pub fn new(client: Session<C>, config: Config) -> Self {
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

        // 4) Fetch UID, FLAGS, INTERNALDATE, X-GM-LABELS, and full header in ONE batch request
        // imap v3 properly supports Gmail extensions like X-GM-LABELS in combined fetch responses
        // NOTE: X-GM-THRID causes server disconnection and is NOT supported
        let fetches = self
            .client
            .fetch(&seq_set, "(UID FLAGS INTERNALDATE X-GM-LABELS RFC822.HEADER)")?;
        debug!("FETCH returned {} records", fetches.len());

        let mut out = Vec::with_capacity(fetches.len());
        for fetch in fetches.iter() {
            let uid = fetch.uid.unwrap_or(0);
            let seq = fetch.message;
            debug!("Parsing FETCH record: seq={}, uid={}", seq, uid);

            // extract full header bytes
            let raw_header = fetch.header().unwrap_or(&[]).to_vec();
            // DEBUG: dump raw headers for diagnostics
            let header_text = String::from_utf8_lossy(&raw_header).into_owned();

            // convert internal date
            let date_str = fetch.internal_date().map(|dt| dt.to_rfc3339()).unwrap_or_default();

            // Labels: use imap v3's gmail_labels() accessor (fetched in batch above)
            // Then add IMAP FLAGS to the label set
            let mut label_set: std::collections::HashSet<String> = fetch
                .gmail_labels()
                .map(|iter| iter.map(String::from).collect())
                .unwrap_or_default();
            for flag in fetch.flags() {
                label_set.insert(flag.to_string());
            }
            let raw_labels: Vec<String> = label_set.into_iter().collect();

            // Thread ID will be computed from standard headers (Message-ID, In-Reply-To, References)
            // after all messages are fetched. Pass None here - thread grouping happens in execute().
            let thread_id: Option<String> = None;

            // build Message
            let msg = Message::new(uid, seq, raw_header, raw_labels, date_str, thread_id);
            debug!(
                "Created message: uid={}, seq={}, subject={}",
                msg.uid, msg.seq, msg.subject
            );

            if msg.from.is_empty() && msg.to.is_empty() && msg.cc.is_empty() {
                error!("UID {} address fields empty. Header was:\n{}", uid, header_text);
            }
            assert!(
                !msg.from.is_empty() || !msg.to.is_empty() || !msg.cc.is_empty(),
                "No address fields (To/Cc/From) for UID {}",
                uid
            );

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

        // Create thread processor (builds thread map using Gmail X-GM-THRID or standard headers)
        let thread_processor = ThreadProcessor::new(&messages);
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
                    message_filter
                        .actions
                        .first()
                        .map(|action| (message_filter.clone(), action.clone()))
                } else {
                    None
                }
            });

            if let Some((matched_filter, action)) = matched {
                info!(
                    "Filter '{}' matched UID {}; applying action {:?}",
                    matched_filter.name, msg.uid, action
                );

                // Process entire thread
                let processed = thread_processor.process_thread_message_filter(&mut self.client, msg, &action)?;

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
        info!("→ Phase 2: applying {} StateFilters", self.state_filters.len());
        let total_messages = messages.len();
        let mut processed_count = 0;
        let mut kept_count = 0;
        let mut expired_count = 0;
        let mut no_match_count = 0;

        let mut i = 0;
        while i < messages.len() {
            processed_count += 1;
            if processed_count % 100 == 0 || processed_count == 1 {
                info!(
                    "  [Phase 2 progress] Processing message {}/{} (kept={}, expired={}, no_match={})",
                    processed_count, total_messages, kept_count, expired_count, no_match_count
                );
            }

            let msg = &messages[i];
            debug!(
                "  Checking UID {} subject='{}' labels={:?}",
                msg.uid,
                &msg.subject[..msg.subject.len().min(50)],
                msg.labels
            );

            if let Some(state_filter) = self.state_filters.iter().find(|sf| sf.matches(msg)) {
                debug!("  → Matched filter '{}'", state_filter.name);

                if let Ttl::Keep = state_filter.ttl {
                    debug!(
                        "  → State '{}' is Keep; protecting UID {} from further filters",
                        state_filter.name, msg.uid
                    );
                    kept_count += 1;
                    messages.remove(i);
                    continue;
                }

                debug!("  → Calling process_thread_state_filter for UID {}", msg.uid);

                // Process entire thread for TTL
                let processed = thread_processor.process_thread_state_filter(
                    &mut self.client,
                    msg,
                    state_filter,
                    &state_filter.action,
                )?;

                if !processed.is_empty() {
                    expired_count += processed.len();
                    debug!("  → Expired {} messages in thread", processed.len());

                    // Remove all processed messages from the list
                    let before_retain = messages.len();
                    messages.retain(|m| !processed.iter().any(|p| p.uid == m.uid));
                    let removed = before_retain - messages.len();
                    debug!(
                        "  → Retained: before={} after={} removed={}",
                        before_retain,
                        messages.len(),
                        removed
                    );
                    // Don't increment i - messages were removed so current index now points to next message
                } else {
                    // TTL not expired yet - move to next message
                    debug!("  → TTL not expired, moving to next message");
                    i += 1;
                }
            } else {
                no_match_count += 1;
                debug!("  → No state filter matched UID {}", msg.uid);
                i += 1;
            }
        }

        info!(
            "  [Phase 2 complete] Total processed: {}, kept: {}, expired: {}, no_match: {}",
            processed_count, kept_count, expired_count, no_match_count
        );
        Ok(())
    }
}
