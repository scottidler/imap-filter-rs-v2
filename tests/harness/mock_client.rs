// tests/harness/mock_client.rs
//
// Mock IMAP client for testing.
// Records all actions for verification and operates against a VirtualMailbox.

use std::sync::{Arc, RwLock};

use crate::harness::virtual_clock::VirtualClock;
use crate::harness::virtual_mailbox::{MailboxMessage, VirtualMailbox};

/// Recorded action types for verification in tests.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordedAction {
    /// Message was starred (\\Starred flag added)
    Star { uid: u32, subject: String },
    /// Message was flagged as important (\\Important flag added)
    Flag { uid: u32, subject: String },
    /// Message was moved from one folder to another
    Move {
        uid: u32,
        from: String,
        to: String,
        subject: String,
    },
    /// Message was marked as deleted
    Delete { uid: u32, subject: String },
    /// A label was added to a message
    AddLabel { uid: u32, label: String },
    /// A label was removed from a message
    RemoveLabel { uid: u32, label: String },
    /// A new label/folder was created
    CreateLabel { label: String },
    /// A mailbox was selected
    Select { mailbox: String },
}

impl RecordedAction {
    /// Check if this is a Star action for the given UID
    pub fn is_star_for(&self, uid: u32) -> bool {
        matches!(self, RecordedAction::Star { uid: u, .. } if *u == uid)
    }

    /// Check if this is a Flag action for the given UID
    pub fn is_flag_for(&self, uid: u32) -> bool {
        matches!(self, RecordedAction::Flag { uid: u, .. } if *u == uid)
    }

    /// Check if this is a Move action to the specified destination
    pub fn is_move_to(&self, destination: &str) -> bool {
        matches!(self, RecordedAction::Move { to, .. } if to == destination)
    }

    /// Check if this is a Delete action for the given UID
    pub fn is_delete_for(&self, uid: u32) -> bool {
        matches!(self, RecordedAction::Delete { uid: u, .. } if *u == uid)
    }
}

/// Mock IMAP client for testing.
/// Operates against a VirtualMailbox and records all actions for verification.
pub struct MockIMAPClient {
    mailbox: Arc<RwLock<VirtualMailbox>>,
    actions: Arc<RwLock<Vec<RecordedAction>>>,
    current_folder: String,
    clock: VirtualClock,
}

impl MockIMAPClient {
    /// Create a new mock client with the given mailbox and clock.
    pub fn new(mailbox: Arc<RwLock<VirtualMailbox>>, clock: VirtualClock) -> Self {
        Self {
            mailbox,
            actions: Arc::new(RwLock::new(Vec::new())),
            current_folder: "INBOX".to_string(),
            clock,
        }
    }

    /// Get the current virtual time.
    pub fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.clock.now()
    }

    /// Get the currently selected folder.
    pub fn current_folder(&self) -> &str {
        &self.current_folder
    }

    // ===== IMAP Operations =====

    /// Select a mailbox/folder.
    pub fn select(&mut self, folder: &str) -> Result<(), String> {
        self.current_folder = folder.to_string();
        self.record_action(RecordedAction::Select {
            mailbox: folder.to_string(),
        });
        Ok(())
    }

    /// Search for messages in the current folder (returns all UIDs).
    pub fn search_all(&self) -> Result<Vec<u32>, String> {
        let mailbox = self.mailbox.read().unwrap();
        let uids: Vec<u32> = mailbox
            .get_messages_with_label(&self.current_folder)
            .iter()
            .map(|m| m.uid)
            .collect();
        Ok(uids)
    }

    /// Fetch all messages in the current folder.
    pub fn fetch_messages(&self) -> Result<Vec<MailboxMessage>, String> {
        let mailbox = self.mailbox.read().unwrap();
        let messages: Vec<MailboxMessage> = mailbox
            .get_messages_with_label(&self.current_folder)
            .into_iter()
            .cloned()
            .collect();
        Ok(messages)
    }

    /// Get a specific message by UID.
    // TEMPORARY: Will be used in Phase 3+ for message inspection in integration tests
    #[allow(dead_code)]
    pub fn get_message(&self, uid: u32) -> Option<MailboxMessage> {
        let mailbox = self.mailbox.read().unwrap();
        mailbox.get_message(uid).cloned()
    }

    /// Get labels for a message.
    pub fn get_labels(&self, uid: u32) -> Result<Vec<String>, String> {
        let mailbox = self.mailbox.read().unwrap();
        if let Some(msg) = mailbox.get_message(uid) {
            Ok(msg.labels.iter().cloned().collect())
        } else {
            Ok(vec![])
        }
    }

    /// Add a flag/label to a message.
    pub fn uid_store_add_flags(&mut self, uid: u32, flag: &str) -> Result<(), String> {
        let subject = self.get_subject(uid);

        let action = if flag == "\\Starred" {
            RecordedAction::Star {
                uid,
                subject: subject.clone(),
            }
        } else if flag == "\\Important" {
            RecordedAction::Flag {
                uid,
                subject: subject.clone(),
            }
        } else if flag == "\\Deleted" {
            let mut mailbox = self.mailbox.write().unwrap();
            mailbox.delete_message(uid);
            RecordedAction::Delete { uid, subject }
        } else {
            RecordedAction::AddLabel {
                uid,
                label: flag.to_string(),
            }
        };

        {
            let mut mailbox = self.mailbox.write().unwrap();
            mailbox.add_label(uid, flag);
        }

        self.record_action(action);
        Ok(())
    }

    /// Remove a flag/label from a message.
    pub fn uid_store_remove_flags(&mut self, uid: u32, flag: &str) -> Result<(), String> {
        {
            let mut mailbox = self.mailbox.write().unwrap();
            mailbox.remove_label(uid, flag);
        }

        self.record_action(RecordedAction::RemoveLabel {
            uid,
            label: flag.to_string(),
        });
        Ok(())
    }

    /// Move a message to another folder.
    pub fn uid_move(&mut self, uid: u32, destination: &str) -> Result<(), String> {
        let subject = self.get_subject(uid);

        // Ensure destination exists
        self.ensure_label(destination)?;

        let action = RecordedAction::Move {
            uid,
            from: self.current_folder.clone(),
            to: destination.to_string(),
            subject,
        };

        {
            let mut mailbox = self.mailbox.write().unwrap();
            mailbox.move_message(uid, &self.current_folder, destination);
        }

        self.record_action(action);
        Ok(())
    }

    /// Ensure a label/folder exists, creating it if necessary.
    pub fn ensure_label(&mut self, label: &str) -> Result<(), String> {
        let exists = {
            let mailbox = self.mailbox.read().unwrap();
            mailbox.label_exists(label)
        };

        if !exists {
            let mut mailbox = self.mailbox.write().unwrap();
            mailbox.create_label(label);
            self.record_action(RecordedAction::CreateLabel {
                label: label.to_string(),
            });
        }

        Ok(())
    }

    /// Check if a label exists.
    pub fn label_exists(&self, label: &str) -> bool {
        let mailbox = self.mailbox.read().unwrap();
        mailbox.label_exists(label)
    }

    /// Simulate logout (no-op for mock).
    // TEMPORARY: Will be used in Phase 3+ when full filter execution is tested
    #[allow(dead_code)]
    pub fn logout(&mut self) -> Result<(), String> {
        Ok(())
    }

    // ===== Action Recording =====

    /// Get all recorded actions.
    pub fn get_recorded_actions(&self) -> Vec<RecordedAction> {
        self.actions.read().unwrap().clone()
    }

    /// Clear all recorded actions.
    pub fn clear_recorded_actions(&self) {
        self.actions.write().unwrap().clear();
    }

    /// Get the count of recorded actions.
    pub fn action_count(&self) -> usize {
        self.actions.read().unwrap().len()
    }

    /// Check if a specific action was recorded.
    pub fn has_action(&self, action: &RecordedAction) -> bool {
        self.actions.read().unwrap().contains(action)
    }

    /// Get all Star actions.
    pub fn get_star_actions(&self) -> Vec<RecordedAction> {
        self.actions
            .read()
            .unwrap()
            .iter()
            .filter(|a| matches!(a, RecordedAction::Star { .. }))
            .cloned()
            .collect()
    }

    /// Get all Move actions.
    pub fn get_move_actions(&self) -> Vec<RecordedAction> {
        self.actions
            .read()
            .unwrap()
            .iter()
            .filter(|a| matches!(a, RecordedAction::Move { .. }))
            .cloned()
            .collect()
    }

    /// Get all Delete actions.
    pub fn get_delete_actions(&self) -> Vec<RecordedAction> {
        self.actions
            .read()
            .unwrap()
            .iter()
            .filter(|a| matches!(a, RecordedAction::Delete { .. }))
            .cloned()
            .collect()
    }

    // ===== Helper Methods =====

    fn record_action(&self, action: RecordedAction) {
        self.actions.write().unwrap().push(action);
    }

    fn get_subject(&self, uid: u32) -> String {
        let mailbox = self.mailbox.read().unwrap();
        mailbox.get_message(uid).map(|m| m.subject.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_client() -> (MockIMAPClient, Arc<RwLock<VirtualMailbox>>) {
        let mailbox = Arc::new(RwLock::new(VirtualMailbox::new()));
        let clock = VirtualClock::new();
        let client = MockIMAPClient::new(Arc::clone(&mailbox), clock);
        (client, mailbox)
    }

    fn add_test_message(mailbox: &Arc<RwLock<VirtualMailbox>>, subject: &str) -> u32 {
        let msg = MailboxMessage::new(
            0,
            subject,
            "sender@example.com",
            "recipient@example.com",
            "2024-01-15T10:00:00+00:00",
        )
        .with_labels(&["INBOX"]);

        mailbox.write().unwrap().add_message(msg)
    }

    #[test]
    fn test_new_client_starts_in_inbox() {
        let (client, _) = setup_test_client();
        assert_eq!(client.current_folder(), "INBOX");
    }

    #[test]
    fn test_select_changes_folder() {
        let (mut client, _) = setup_test_client();
        client.select("Purgatory").unwrap();
        assert_eq!(client.current_folder(), "Purgatory");

        let actions = client.get_recorded_actions();
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            RecordedAction::Select { mailbox } if mailbox == "Purgatory"
        ));
    }

    #[test]
    fn test_search_all_returns_uids() {
        let (client, mailbox) = setup_test_client();

        let uid1 = add_test_message(&mailbox, "Message 1");
        let uid2 = add_test_message(&mailbox, "Message 2");

        let uids = client.search_all().unwrap();
        assert_eq!(uids.len(), 2);
        assert!(uids.contains(&uid1));
        assert!(uids.contains(&uid2));
    }

    #[test]
    fn test_fetch_messages_returns_messages() {
        let (client, mailbox) = setup_test_client();

        add_test_message(&mailbox, "Test Subject");

        let messages = client.fetch_messages().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].subject, "Test Subject");
    }

    #[test]
    fn test_uid_store_star() {
        let (mut client, mailbox) = setup_test_client();
        let uid = add_test_message(&mailbox, "Starred Message");

        client.uid_store_add_flags(uid, "\\Starred").unwrap();

        // Check action recorded
        let actions = client.get_recorded_actions();
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            RecordedAction::Star { uid: u, subject } if *u == uid && subject == "Starred Message"
        ));

        // Check label applied
        let labels = client.get_labels(uid).unwrap();
        assert!(labels.contains(&"\\Starred".to_string()));
    }

    #[test]
    fn test_uid_store_flag_important() {
        let (mut client, mailbox) = setup_test_client();
        let uid = add_test_message(&mailbox, "Important Message");

        client.uid_store_add_flags(uid, "\\Important").unwrap();

        let actions = client.get_recorded_actions();
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            RecordedAction::Flag { uid: u, .. } if *u == uid
        ));
    }

    #[test]
    fn test_uid_store_delete() {
        let (mut client, mailbox) = setup_test_client();
        let uid = add_test_message(&mailbox, "Deleted Message");

        client.uid_store_add_flags(uid, "\\Deleted").unwrap();

        let actions = client.get_recorded_actions();
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            RecordedAction::Delete { uid: u, .. } if *u == uid
        ));

        // Message should be marked deleted in mailbox
        let msg = mailbox.read().unwrap().get_message(uid).unwrap().clone();
        assert!(msg.deleted);
    }

    #[test]
    fn test_uid_store_custom_label() {
        let (mut client, mailbox) = setup_test_client();
        let uid = add_test_message(&mailbox, "Custom Label Message");

        client.uid_store_add_flags(uid, "CustomLabel").unwrap();

        let actions = client.get_recorded_actions();
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            RecordedAction::AddLabel { uid: u, label } if *u == uid && label == "CustomLabel"
        ));
    }

    #[test]
    fn test_uid_store_remove_flags() {
        let (mut client, mailbox) = setup_test_client();
        let uid = add_test_message(&mailbox, "Test Message");

        // Add then remove a label
        client.uid_store_add_flags(uid, "Starred").unwrap();
        client.uid_store_remove_flags(uid, "Starred").unwrap();

        let actions = client.get_recorded_actions();
        assert_eq!(actions.len(), 2);
        assert!(matches!(
            &actions[1],
            RecordedAction::RemoveLabel { uid: u, label } if *u == uid && label == "Starred"
        ));
    }

    #[test]
    fn test_uid_move() {
        let (mut client, mailbox) = setup_test_client();
        let uid = add_test_message(&mailbox, "Moving Message");

        client.uid_move(uid, "Purgatory").unwrap();

        let actions = client.get_recorded_actions();
        // Should have CreateLabel (for Purgatory) + Move
        assert!(!actions.is_empty());

        let move_actions = client.get_move_actions();
        assert_eq!(move_actions.len(), 1);
        assert!(matches!(
            &move_actions[0],
            RecordedAction::Move { uid: u, from, to, .. }
                if *u == uid && from == "INBOX" && to == "Purgatory"
        ));

        // Message should be in Purgatory now
        let msg = mailbox.read().unwrap().get_message(uid).unwrap().clone();
        assert!(msg.labels.contains("Purgatory"));
        assert!(!msg.labels.contains("INBOX"));
    }

    #[test]
    fn test_ensure_label_creates_if_not_exists() {
        let (mut client, _) = setup_test_client();

        assert!(!client.label_exists("NewLabel"));
        client.ensure_label("NewLabel").unwrap();
        assert!(client.label_exists("NewLabel"));

        let actions = client.get_recorded_actions();
        assert!(actions
            .iter()
            .any(|a| matches!(a, RecordedAction::CreateLabel { label } if label == "NewLabel")));
    }

    #[test]
    fn test_ensure_label_does_not_duplicate() {
        let (mut client, _) = setup_test_client();

        client.ensure_label("INBOX").unwrap(); // Already exists
        let actions = client.get_recorded_actions();
        assert!(actions.is_empty()); // No CreateLabel action
    }

    #[test]
    fn test_clear_recorded_actions() {
        let (mut client, mailbox) = setup_test_client();
        let uid = add_test_message(&mailbox, "Test");

        client.uid_store_add_flags(uid, "\\Starred").unwrap();
        assert_eq!(client.action_count(), 1);

        client.clear_recorded_actions();
        assert_eq!(client.action_count(), 0);
    }

    #[test]
    fn test_has_action() {
        let (mut client, mailbox) = setup_test_client();
        let uid = add_test_message(&mailbox, "Test Subject");

        client.uid_store_add_flags(uid, "\\Starred").unwrap();

        assert!(client.has_action(&RecordedAction::Star {
            uid,
            subject: "Test Subject".to_string()
        }));

        assert!(!client.has_action(&RecordedAction::Delete {
            uid,
            subject: "Test Subject".to_string()
        }));
    }

    #[test]
    fn test_recorded_action_helpers() {
        let star = RecordedAction::Star {
            uid: 1,
            subject: "Test".to_string(),
        };
        assert!(star.is_star_for(1));
        assert!(!star.is_star_for(2));

        let flag = RecordedAction::Flag {
            uid: 2,
            subject: "Test".to_string(),
        };
        assert!(flag.is_flag_for(2));

        let mov = RecordedAction::Move {
            uid: 3,
            from: "INBOX".to_string(),
            to: "Purgatory".to_string(),
            subject: "Test".to_string(),
        };
        assert!(mov.is_move_to("Purgatory"));
        assert!(!mov.is_move_to("Archive"));

        let del = RecordedAction::Delete {
            uid: 4,
            subject: "Test".to_string(),
        };
        assert!(del.is_delete_for(4));
    }

    #[test]
    fn test_get_filtered_actions() {
        let (mut client, mailbox) = setup_test_client();

        let uid1 = add_test_message(&mailbox, "Msg 1");
        let uid2 = add_test_message(&mailbox, "Msg 2");
        let uid3 = add_test_message(&mailbox, "Msg 3");

        client.uid_store_add_flags(uid1, "\\Starred").unwrap();
        client.uid_move(uid2, "Purgatory").unwrap();
        client.uid_store_add_flags(uid3, "\\Deleted").unwrap();

        assert_eq!(client.get_star_actions().len(), 1);
        assert_eq!(client.get_move_actions().len(), 1);
        assert_eq!(client.get_delete_actions().len(), 1);
    }

    #[test]
    fn test_client_with_virtual_clock() {
        let mailbox = Arc::new(RwLock::new(VirtualMailbox::new()));
        let clock = VirtualClock::at(
            chrono::DateTime::parse_from_rfc3339("2024-06-15T12:00:00+00:00")
                .unwrap()
                .with_timezone(&chrono::Utc),
        );
        let client = MockIMAPClient::new(mailbox, clock.clone());

        let expected = chrono::DateTime::parse_from_rfc3339("2024-06-15T12:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);

        assert_eq!(client.now(), expected);

        // Advance clock
        clock.advance_days(7);
        let expected_after = chrono::DateTime::parse_from_rfc3339("2024-06-22T12:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);

        assert_eq!(client.now(), expected_after);
    }
}
