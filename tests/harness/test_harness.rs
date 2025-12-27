// tests/harness/test_harness.rs
//
// High-level test harness combining all components.
// Provides a convenient API for writing integration tests.

use std::sync::{Arc, RwLock};

use crate::harness::fixtures::{EmailFixture, FixtureLoader};
use crate::harness::mock_client::{MockIMAPClient, RecordedAction};
use crate::harness::virtual_clock::VirtualClock;
use crate::harness::virtual_mailbox::{MailboxMessage, VirtualMailbox};

/// High-level test harness combining all components.
/// Provides a convenient API for writing integration tests.
pub struct TestHarness {
    pub mailbox: Arc<RwLock<VirtualMailbox>>,
    pub clock: VirtualClock,
    pub client: MockIMAPClient,
    // TEMPORARY: loader will be used in Phase 4+ for fixture-based tests
    #[allow(dead_code)]
    loader: FixtureLoader,
}

impl TestHarness {
    /// Create a new test harness with default configuration.
    pub fn new() -> Self {
        let mailbox = Arc::new(RwLock::new(VirtualMailbox::new()));
        let clock = VirtualClock::new();
        let client = MockIMAPClient::new(Arc::clone(&mailbox), clock.clone());
        let loader = FixtureLoader::new();

        Self {
            mailbox,
            clock,
            client,
            loader,
        }
    }

    /// Create a test harness with a specific starting time.
    pub fn at_time(time: chrono::DateTime<chrono::Utc>) -> Self {
        let mailbox = Arc::new(RwLock::new(VirtualMailbox::new()));
        let clock = VirtualClock::at(time);
        let client = MockIMAPClient::new(Arc::clone(&mailbox), clock.clone());
        let loader = FixtureLoader::new();

        Self {
            mailbox,
            clock,
            client,
            loader,
        }
    }

    // ===== Message Management =====

    /// Add a message directly to the mailbox.
    pub fn add_message(&mut self, message: MailboxMessage) -> u32 {
        self.mailbox.write().unwrap().add_message(message)
    }

    /// Add a message with specific labels.
    pub fn add_message_with_labels(&mut self, mut message: MailboxMessage, labels: &[&str]) -> u32 {
        for label in labels {
            message.labels.insert(label.to_string());
        }
        self.mailbox.write().unwrap().add_message(message)
    }

    // TEMPORARY: These fixture methods will be used in Phase 4+ for fixture-based integration tests
    #[allow(dead_code)]
    /// Load a fixture email and add it to the mailbox.
    pub fn add_fixture(&mut self, fixture_path: &str) -> Result<u32, String> {
        let fixture = self.loader.load_email(fixture_path).map_err(|e| e.to_string())?;
        Ok(self.add_message(fixture.message))
    }

    #[allow(dead_code)]
    /// Load a fixture with specific labels.
    pub fn add_fixture_with_labels(&mut self, fixture_path: &str, labels: &[&str]) -> Result<u32, String> {
        let fixture = self.loader.load_email(fixture_path).map_err(|e| e.to_string())?;
        Ok(self.add_message_with_labels(fixture.message, labels))
    }

    #[allow(dead_code)]
    /// Load a fixture with a specific internal date (days ago from current virtual time).
    pub fn add_fixture_dated(&mut self, fixture_path: &str, labels: &[&str], days_ago: i64) -> Result<u32, String> {
        let mut fixture = self.loader.load_email(fixture_path).map_err(|e| e.to_string())?;

        // Set the internal date to `days_ago` days before current virtual time
        let date = self.clock.now() - chrono::Duration::days(days_ago);
        fixture.message.date = date.to_rfc3339();

        Ok(self.add_message_with_labels(fixture.message, labels))
    }

    #[allow(dead_code)]
    /// Load all fixtures from a directory.
    pub fn load_fixtures_from_directory(&mut self, dir_path: &str) -> Result<Vec<EmailFixture>, String> {
        self.loader.load_directory(dir_path).map_err(|e| e.to_string())
    }

    // ===== Time Control =====

    /// Advance virtual time by the given number of days.
    pub fn advance_days(&self, days: i64) {
        self.clock.advance_days(days);
    }

    // TEMPORARY: Will be used in Phase 4+ for more granular time control tests
    #[allow(dead_code)]
    /// Advance virtual time by the given duration.
    pub fn advance(&self, duration: chrono::Duration) {
        self.clock.advance(duration);
    }

    /// Get the current virtual time.
    pub fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.clock.now()
    }

    // ===== Action Inspection =====

    /// Get all recorded actions.
    pub fn actions(&self) -> Vec<RecordedAction> {
        self.client.get_recorded_actions()
    }

    /// Clear all recorded actions.
    pub fn clear_actions(&self) {
        self.client.clear_recorded_actions();
    }

    /// Get the count of recorded actions.
    pub fn action_count(&self) -> usize {
        self.client.action_count()
    }

    /// Get all Star actions.
    pub fn star_actions(&self) -> Vec<RecordedAction> {
        self.client.get_star_actions()
    }

    /// Get all Move actions.
    pub fn move_actions(&self) -> Vec<RecordedAction> {
        self.client.get_move_actions()
    }

    /// Get all Delete actions.
    pub fn delete_actions(&self) -> Vec<RecordedAction> {
        self.client.get_delete_actions()
    }

    // ===== Mailbox State Inspection =====

    /// Get the count of non-deleted messages in a label/folder.
    pub fn message_count(&self, label: &str) -> usize {
        self.mailbox.read().unwrap().get_messages_with_label(label).len()
    }

    /// Get the total count of non-deleted messages.
    pub fn total_message_count(&self) -> usize {
        self.mailbox.read().unwrap().message_count()
    }

    // TEMPORARY: Will be used in Phase 4+ for label existence assertions
    #[allow(dead_code)]
    /// Check if a label exists.
    pub fn label_exists(&self, label: &str) -> bool {
        self.mailbox.read().unwrap().label_exists(label)
    }

    /// Get a message by UID.
    pub fn get_message(&self, uid: u32) -> Option<MailboxMessage> {
        self.mailbox.read().unwrap().get_message(uid).cloned()
    }

    // ===== Assertion Helpers =====

    // TEMPORARY: Will be used in Phase 4+ for specific action assertions
    #[allow(dead_code)]
    /// Assert that a specific action was recorded.
    pub fn assert_action(&self, expected: &RecordedAction) {
        let actions = self.actions();
        assert!(
            actions.contains(expected),
            "Expected action {:?} not found in {:?}",
            expected,
            actions
        );
    }

    /// Assert that no actions were recorded.
    pub fn assert_no_actions(&self) {
        let actions = self.actions();
        assert!(actions.is_empty(), "Expected no actions but found {:?}", actions);
    }

    /// Assert that a message was starred.
    pub fn assert_starred(&self, uid: u32) {
        let star_actions = self.star_actions();
        assert!(
            star_actions.iter().any(|a| a.is_star_for(uid)),
            "Expected UID {} to be starred, but star actions were: {:?}",
            uid,
            star_actions
        );
    }

    /// Assert that a message was moved to a destination.
    pub fn assert_moved_to(&self, uid: u32, destination: &str) {
        let move_actions = self.move_actions();
        let found = move_actions
            .iter()
            .any(|a| matches!(a, RecordedAction::Move { uid: u, to, .. } if *u == uid && to == destination));
        assert!(
            found,
            "Expected UID {} to be moved to {}, but move actions were: {:?}",
            uid, destination, move_actions
        );
    }

    /// Assert that a message was deleted.
    pub fn assert_deleted(&self, uid: u32) {
        let delete_actions = self.delete_actions();
        assert!(
            delete_actions.iter().any(|a| a.is_delete_for(uid)),
            "Expected UID {} to be deleted, but delete actions were: {:?}",
            uid,
            delete_actions
        );
    }

    /// Assert that the message count in a label matches expected.
    pub fn assert_message_count(&self, label: &str, expected: usize) {
        let actual = self.message_count(label);
        assert_eq!(
            actual, expected,
            "Expected {} messages in '{}', found {}",
            expected, label, actual
        );
    }

    /// Assert that a message has a specific label.
    pub fn assert_has_label(&self, uid: u32, label: &str) {
        let msg = self
            .get_message(uid)
            .unwrap_or_else(|| panic!("Message with UID {} not found", uid));
        assert!(
            msg.labels.contains(label),
            "Expected UID {} to have label '{}', but labels were: {:?}",
            uid,
            label,
            msg.labels
        );
    }

    /// Assert that a message does NOT have a specific label.
    pub fn assert_not_has_label(&self, uid: u32, label: &str) {
        let msg = self
            .get_message(uid)
            .unwrap_or_else(|| panic!("Message with UID {} not found", uid));
        assert!(
            !msg.labels.contains(label),
            "Expected UID {} to NOT have label '{}', but it does. Labels: {:?}",
            uid,
            label,
            msg.labels
        );
    }
}

impl Default for TestHarness {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_test_message(subject: &str) -> MailboxMessage {
        MailboxMessage::new(
            0,
            subject,
            "sender@example.com",
            "recipient@example.com",
            &Utc::now().to_rfc3339(),
        )
    }

    #[test]
    fn test_harness_new() {
        let harness = TestHarness::new();
        assert_eq!(harness.total_message_count(), 0);
        assert_eq!(harness.action_count(), 0);
    }

    #[test]
    fn test_harness_at_time() {
        let specific_time = chrono::DateTime::parse_from_rfc3339("2024-06-15T12:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let harness = TestHarness::at_time(specific_time);
        assert_eq!(harness.now(), specific_time);
    }

    #[test]
    fn test_add_message() {
        let mut harness = TestHarness::new();
        let msg = make_test_message("Test Subject");

        let uid = harness.add_message(msg);
        assert_eq!(uid, 1);
        assert_eq!(harness.total_message_count(), 1);
    }

    #[test]
    fn test_add_message_with_labels() {
        let mut harness = TestHarness::new();
        let msg = make_test_message("Test Subject");

        let uid = harness.add_message_with_labels(msg, &["INBOX", "Starred"]);

        harness.assert_has_label(uid, "INBOX");
        harness.assert_has_label(uid, "Starred");
    }

    #[test]
    fn test_advance_days() {
        let start = chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let harness = TestHarness::at_time(start);
        harness.advance_days(7);

        let expected = chrono::DateTime::parse_from_rfc3339("2024-01-08T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        assert_eq!(harness.now(), expected);
    }

    #[test]
    fn test_client_actions_are_recorded() {
        let mut harness = TestHarness::new();
        let msg = make_test_message("Test").with_labels(&["INBOX"]);
        let uid = harness.add_message(msg);

        harness.client.uid_store_add_flags(uid, "\\Starred").unwrap();

        assert_eq!(harness.action_count(), 1);
        harness.assert_starred(uid);
    }

    #[test]
    fn test_clear_actions() {
        let mut harness = TestHarness::new();
        let msg = make_test_message("Test").with_labels(&["INBOX"]);
        let uid = harness.add_message(msg);

        harness.client.uid_store_add_flags(uid, "\\Starred").unwrap();
        assert_eq!(harness.action_count(), 1);

        harness.clear_actions();
        assert_eq!(harness.action_count(), 0);
    }

    #[test]
    fn test_assert_moved_to() {
        let mut harness = TestHarness::new();
        let msg = make_test_message("Test").with_labels(&["INBOX"]);
        let uid = harness.add_message(msg);

        harness.client.uid_move(uid, "Purgatory").unwrap();

        harness.assert_moved_to(uid, "Purgatory");
        harness.assert_not_has_label(uid, "INBOX");
        harness.assert_has_label(uid, "Purgatory");
    }

    #[test]
    fn test_assert_deleted() {
        let mut harness = TestHarness::new();
        let msg = make_test_message("Test").with_labels(&["INBOX"]);
        let uid = harness.add_message(msg);

        harness.client.uid_store_add_flags(uid, "\\Deleted").unwrap();

        harness.assert_deleted(uid);
    }

    #[test]
    fn test_message_count() {
        let mut harness = TestHarness::new();

        let msg1 = make_test_message("Msg 1").with_labels(&["INBOX"]);
        let msg2 = make_test_message("Msg 2").with_labels(&["INBOX", "Starred"]);
        let msg3 = make_test_message("Msg 3").with_labels(&["Archive"]);

        harness.add_message(msg1);
        harness.add_message(msg2);
        harness.add_message(msg3);

        harness.assert_message_count("INBOX", 2);
        harness.assert_message_count("Starred", 1);
        harness.assert_message_count("Archive", 1);
    }

    #[test]
    fn test_assert_no_actions() {
        let harness = TestHarness::new();
        harness.assert_no_actions(); // Should not panic
    }

    #[test]
    #[should_panic(expected = "Expected no actions")]
    fn test_assert_no_actions_fails_when_actions_exist() {
        let mut harness = TestHarness::new();
        let msg = make_test_message("Test").with_labels(&["INBOX"]);
        let uid = harness.add_message(msg);

        harness.client.uid_store_add_flags(uid, "\\Starred").unwrap();
        harness.assert_no_actions(); // Should panic
    }

    #[test]
    fn test_full_workflow() {
        let mut harness = TestHarness::at_time(
            chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00+00:00")
                .unwrap()
                .with_timezone(&Utc),
        );

        // Add messages
        let msg1 = make_test_message("Important Email").with_labels(&["INBOX"]);
        let msg2 = make_test_message("Old Newsletter").with_labels(&["INBOX"]);

        let uid1 = harness.add_message(msg1);
        let uid2 = harness.add_message(msg2);

        // Star the important one
        harness.client.uid_store_add_flags(uid1, "\\Starred").unwrap();

        // Move the old one to Purgatory
        harness.client.uid_move(uid2, "Purgatory").unwrap();

        // Verify
        harness.assert_starred(uid1);
        harness.assert_moved_to(uid2, "Purgatory");
        harness.assert_message_count("INBOX", 1);
        harness.assert_message_count("Purgatory", 1);

        // Advance time
        harness.advance_days(7);

        // Clear and do more actions
        harness.clear_actions();
        harness.client.select("Purgatory").unwrap();
        harness.client.uid_move(uid2, "Oblivion").unwrap();

        harness.assert_moved_to(uid2, "Oblivion");
        harness.assert_message_count("Purgatory", 0);
        harness.assert_message_count("Oblivion", 1);
    }
}
