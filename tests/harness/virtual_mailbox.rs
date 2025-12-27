// tests/harness/virtual_mailbox.rs
//
// In-memory IMAP mailbox for testing.
// Simulates an IMAP server's mailbox state without network access.

use std::collections::{HashMap, HashSet};

/// Represents the state of a message in the virtual mailbox.
#[derive(Debug, Clone)]
pub struct MailboxMessage {
    pub uid: u32,
    pub seq: u32,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub from: Vec<String>,
    pub subject: String,
    // TEMPORARY: Will be used in Phase 2+ for TTL evaluation
    #[allow(dead_code)]
    pub date: String,
    pub labels: HashSet<String>,
    pub flags: HashSet<String>,
    pub headers: HashMap<String, String>,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub thread_id: Option<String>,
    pub deleted: bool,
}

impl MailboxMessage {
    /// Create a new mailbox message with minimal required fields.
    pub fn new(uid: u32, subject: &str, from: &str, to: &str, date: &str) -> Self {
        Self {
            uid,
            seq: uid,
            to: vec![to.to_string()],
            cc: Vec::new(),
            from: vec![from.to_string()],
            subject: subject.to_string(),
            date: date.to_string(),
            labels: HashSet::new(),
            flags: HashSet::new(),
            headers: HashMap::new(),
            message_id: None,
            in_reply_to: None,
            references: Vec::new(),
            thread_id: None,
            deleted: false,
        }
    }

    /// Builder method to add labels.
    pub fn with_labels(mut self, labels: &[&str]) -> Self {
        for label in labels {
            self.labels.insert(label.to_string());
        }
        self
    }

    /// Builder method to set message ID (for threading).
    pub fn with_message_id(mut self, message_id: &str) -> Self {
        self.message_id = Some(message_id.to_string());
        self
    }

    /// Builder method to set in-reply-to (for threading).
    pub fn with_in_reply_to(mut self, in_reply_to: &str) -> Self {
        self.in_reply_to = Some(in_reply_to.to_string());
        self
    }

    /// Builder method to set references (for threading).
    pub fn with_references(mut self, refs: &[&str]) -> Self {
        self.references = refs.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Builder method to set thread ID (Gmail X-GM-THRID).
    pub fn with_thread_id(mut self, thread_id: &str) -> Self {
        self.thread_id = Some(thread_id.to_string());
        self
    }

    /// Builder method to add CC recipients.
    pub fn with_cc(mut self, cc: &[&str]) -> Self {
        self.cc = cc.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Builder method to add a header.
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.insert(name.to_string(), value.to_string());
        self
    }
}

/// Record of a message move operation.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveRecord {
    pub uid: u32,
    pub from_label: String,
    pub to_label: String,
}

/// In-memory IMAP mailbox for testing.
#[derive(Debug, Default)]
pub struct VirtualMailbox {
    messages: HashMap<u32, MailboxMessage>,
    next_uid: u32,
    labels: HashSet<String>,
    moves: Vec<MoveRecord>,
}

impl VirtualMailbox {
    /// Create a new empty virtual mailbox with standard labels.
    pub fn new() -> Self {
        let mut labels = HashSet::new();
        labels.insert("INBOX".to_string());
        labels.insert("\\Starred".to_string());
        labels.insert("\\Important".to_string());
        labels.insert("Starred".to_string());
        labels.insert("Important".to_string());

        Self {
            messages: HashMap::new(),
            next_uid: 1,
            labels,
            moves: Vec::new(),
        }
    }

    /// Add a message to the mailbox, returning the assigned UID.
    pub fn add_message(&mut self, mut message: MailboxMessage) -> u32 {
        let uid = self.next_uid;
        self.next_uid += 1;

        message.uid = uid;
        message.seq = uid;

        self.messages.insert(uid, message);
        uid
    }

    /// Get a message by UID.
    pub fn get_message(&self, uid: u32) -> Option<&MailboxMessage> {
        self.messages.get(&uid)
    }

    /// Get a mutable reference to a message by UID.
    // TEMPORARY: Will be used in Phase 2+ for message modification in tests
    #[allow(dead_code)]
    pub fn get_message_mut(&mut self, uid: u32) -> Option<&mut MailboxMessage> {
        self.messages.get_mut(&uid)
    }

    /// Get all non-deleted messages.
    pub fn get_all_messages(&self) -> Vec<&MailboxMessage> {
        self.messages.values().filter(|m| !m.deleted).collect()
    }

    /// Get messages with a specific label.
    pub fn get_messages_with_label(&self, label: &str) -> Vec<&MailboxMessage> {
        self.messages
            .values()
            .filter(|m| !m.deleted && m.labels.contains(label))
            .collect()
    }

    /// Add a label to a message.
    pub fn add_label(&mut self, uid: u32, label: &str) -> bool {
        if let Some(msg) = self.messages.get_mut(&uid) {
            msg.labels.insert(label.to_string());
            true
        } else {
            false
        }
    }

    /// Remove a label from a message.
    pub fn remove_label(&mut self, uid: u32, label: &str) -> bool {
        if let Some(msg) = self.messages.get_mut(&uid) {
            msg.labels.remove(label);
            true
        } else {
            false
        }
    }

    /// Move a message from one folder to another.
    pub fn move_message(&mut self, uid: u32, from: &str, to: &str) -> bool {
        if let Some(msg) = self.messages.get_mut(&uid) {
            msg.labels.remove(from);
            msg.labels.insert(to.to_string());

            self.moves.push(MoveRecord {
                uid,
                from_label: from.to_string(),
                to_label: to.to_string(),
            });

            // Ensure destination label exists
            self.labels.insert(to.to_string());

            true
        } else {
            false
        }
    }

    /// Mark a message as deleted.
    pub fn delete_message(&mut self, uid: u32) -> bool {
        if let Some(msg) = self.messages.get_mut(&uid) {
            msg.deleted = true;
            msg.flags.insert("\\Deleted".to_string());
            true
        } else {
            false
        }
    }

    /// Expunge deleted messages (actually remove them).
    pub fn expunge(&mut self) -> Vec<u32> {
        let deleted: Vec<u32> = self
            .messages
            .iter()
            .filter(|(_, m)| m.deleted)
            .map(|(uid, _)| *uid)
            .collect();

        for uid in &deleted {
            self.messages.remove(uid);
        }

        deleted
    }

    /// Get the move history for assertions.
    pub fn get_move_history(&self) -> &[MoveRecord] {
        &self.moves
    }

    /// Check if a label/folder exists.
    pub fn label_exists(&self, label: &str) -> bool {
        self.labels.contains(label)
    }

    /// Create a label.
    pub fn create_label(&mut self, label: &str) {
        self.labels.insert(label.to_string());
    }

    /// Get the count of non-deleted messages.
    pub fn message_count(&self) -> usize {
        self.messages.values().filter(|m| !m.deleted).count()
    }

    /// Clear the move history.
    // TEMPORARY: Will be used in Phase 2+ for multi-step test scenarios
    #[allow(dead_code)]
    pub fn clear_move_history(&mut self) {
        self.moves.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_message() -> MailboxMessage {
        MailboxMessage::new(
            0, // UID will be assigned by mailbox
            "Test Subject",
            "sender@example.com",
            "recipient@example.com",
            "2024-01-15T10:00:00+00:00",
        )
    }

    #[test]
    fn test_new_mailbox_has_standard_labels() {
        let mailbox = VirtualMailbox::new();
        assert!(mailbox.label_exists("INBOX"));
        assert!(mailbox.label_exists("\\Starred"));
        assert!(mailbox.label_exists("\\Important"));
    }

    #[test]
    fn test_add_message_assigns_uid() {
        let mut mailbox = VirtualMailbox::new();
        let msg = make_test_message();

        let uid1 = mailbox.add_message(msg.clone());
        let uid2 = mailbox.add_message(msg);

        assert_eq!(uid1, 1);
        assert_eq!(uid2, 2);
    }

    #[test]
    fn test_get_message_by_uid() {
        let mut mailbox = VirtualMailbox::new();
        let msg = make_test_message();
        let uid = mailbox.add_message(msg);

        let retrieved = mailbox.get_message(uid);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().subject, "Test Subject");
    }

    #[test]
    fn test_get_nonexistent_message_returns_none() {
        let mailbox = VirtualMailbox::new();
        assert!(mailbox.get_message(999).is_none());
    }

    #[test]
    fn test_add_label_to_message() {
        let mut mailbox = VirtualMailbox::new();
        let msg = make_test_message();
        let uid = mailbox.add_message(msg);

        assert!(mailbox.add_label(uid, "INBOX"));

        let retrieved = mailbox.get_message(uid).unwrap();
        assert!(retrieved.labels.contains("INBOX"));
    }

    #[test]
    fn test_remove_label_from_message() {
        let mut mailbox = VirtualMailbox::new();
        let msg = make_test_message().with_labels(&["INBOX", "Important"]);
        let uid = mailbox.add_message(msg);

        assert!(mailbox.remove_label(uid, "INBOX"));

        let retrieved = mailbox.get_message(uid).unwrap();
        assert!(!retrieved.labels.contains("INBOX"));
        assert!(retrieved.labels.contains("Important"));
    }

    #[test]
    fn test_get_messages_with_label() {
        let mut mailbox = VirtualMailbox::new();

        let msg1 = make_test_message().with_labels(&["INBOX"]);
        let msg2 = make_test_message().with_labels(&["INBOX", "Starred"]);
        let msg3 = make_test_message().with_labels(&["Archived"]);

        mailbox.add_message(msg1);
        mailbox.add_message(msg2);
        mailbox.add_message(msg3);

        let inbox_messages = mailbox.get_messages_with_label("INBOX");
        assert_eq!(inbox_messages.len(), 2);

        let starred_messages = mailbox.get_messages_with_label("Starred");
        assert_eq!(starred_messages.len(), 1);
    }

    #[test]
    fn test_move_message() {
        let mut mailbox = VirtualMailbox::new();
        let msg = make_test_message().with_labels(&["INBOX"]);
        let uid = mailbox.add_message(msg);

        assert!(mailbox.move_message(uid, "INBOX", "Purgatory"));

        let retrieved = mailbox.get_message(uid).unwrap();
        assert!(!retrieved.labels.contains("INBOX"));
        assert!(retrieved.labels.contains("Purgatory"));

        // Check move was recorded
        let history = mailbox.get_move_history();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].uid, uid);
        assert_eq!(history[0].from_label, "INBOX");
        assert_eq!(history[0].to_label, "Purgatory");
    }

    #[test]
    fn test_delete_message() {
        let mut mailbox = VirtualMailbox::new();
        let msg = make_test_message();
        let uid = mailbox.add_message(msg);

        assert!(mailbox.delete_message(uid));

        let retrieved = mailbox.get_message(uid).unwrap();
        assert!(retrieved.deleted);
        assert!(retrieved.flags.contains("\\Deleted"));
    }

    #[test]
    fn test_deleted_messages_excluded_from_get_all() {
        let mut mailbox = VirtualMailbox::new();

        let msg1 = make_test_message();
        let msg2 = make_test_message();

        let uid1 = mailbox.add_message(msg1);
        mailbox.add_message(msg2);

        mailbox.delete_message(uid1);

        let all = mailbox.get_all_messages();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn test_expunge_removes_deleted_messages() {
        let mut mailbox = VirtualMailbox::new();

        let msg1 = make_test_message();
        let msg2 = make_test_message();

        let uid1 = mailbox.add_message(msg1);
        let uid2 = mailbox.add_message(msg2);

        mailbox.delete_message(uid1);
        let expunged = mailbox.expunge();

        assert_eq!(expunged, vec![uid1]);
        assert!(mailbox.get_message(uid1).is_none());
        assert!(mailbox.get_message(uid2).is_some());
    }

    #[test]
    fn test_mailbox_message_builder() {
        let msg = MailboxMessage::new(0, "Subject", "from@test.com", "to@test.com", "2024-01-15")
            .with_labels(&["INBOX", "Starred"])
            .with_cc(&["cc@test.com"])
            .with_message_id("<msg-001@test.com>")
            .with_in_reply_to("<parent@test.com>")
            .with_references(&["<root@test.com>", "<parent@test.com>"])
            .with_thread_id("gmail-thread-123")
            .with_header("X-Priority", "1");

        assert!(msg.labels.contains("INBOX"));
        assert!(msg.labels.contains("Starred"));
        assert_eq!(msg.cc, vec!["cc@test.com"]);
        assert_eq!(msg.message_id, Some("<msg-001@test.com>".to_string()));
        assert_eq!(msg.in_reply_to, Some("<parent@test.com>".to_string()));
        assert_eq!(msg.references.len(), 2);
        assert_eq!(msg.thread_id, Some("gmail-thread-123".to_string()));
        assert_eq!(msg.headers.get("X-Priority"), Some(&"1".to_string()));
    }

    #[test]
    fn test_create_label() {
        let mut mailbox = VirtualMailbox::new();
        assert!(!mailbox.label_exists("CustomLabel"));

        mailbox.create_label("CustomLabel");
        assert!(mailbox.label_exists("CustomLabel"));
    }

    #[test]
    fn test_message_count() {
        let mut mailbox = VirtualMailbox::new();
        assert_eq!(mailbox.message_count(), 0);

        let uid1 = mailbox.add_message(make_test_message());
        mailbox.add_message(make_test_message());
        assert_eq!(mailbox.message_count(), 2);

        mailbox.delete_message(uid1);
        assert_eq!(mailbox.message_count(), 1);
    }
}
