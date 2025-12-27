// tests/integration.rs
//
// Integration test entry point for imap-filter-rs-v2.
// This file enables the test harness modules to be compiled and tested.

mod harness;

// Re-export for use in other test files
// TEMPORARY: Some imports may not be used until Phase 4+
#[allow(unused_imports)]
use harness::{
    Clock, EmailFixture, FixtureError, FixtureLoader, MailboxMessage, MockIMAPClient, MoveRecord, RealClock,
    RecordedAction, TestHarness, VirtualClock, VirtualMailbox,
};

#[cfg(test)]
mod harness_tests {
    use super::harness::*;
    use chrono::{Duration, Utc};

    // ===== VirtualClock integration tests =====

    #[test]
    fn test_clock_trait_polymorphism() {
        fn get_time<C: Clock>(clock: &C) -> chrono::DateTime<Utc> {
            clock.now()
        }

        let virtual_clock = VirtualClock::new();
        let real_clock = RealClock;

        // Both should return valid times
        let _vt = get_time(&virtual_clock);
        let _rt = get_time(&real_clock);
    }

    #[test]
    fn test_virtual_clock_time_travel() {
        let clock = VirtualClock::at(
            chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00+00:00")
                .unwrap()
                .with_timezone(&Utc),
        );

        // Go forward 30 days
        clock.advance(Duration::days(30));

        let expected = chrono::DateTime::parse_from_rfc3339("2024-01-31T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        assert_eq!(clock.now(), expected);

        // Go back 10 days
        clock.rewind(Duration::days(10));

        let expected = chrono::DateTime::parse_from_rfc3339("2024-01-21T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        assert_eq!(clock.now(), expected);
    }

    // ===== VirtualMailbox integration tests =====

    #[test]
    fn test_mailbox_full_lifecycle() {
        let mut mailbox = VirtualMailbox::new();

        // Add a message
        let msg = MailboxMessage::new(
            0,
            "Test Email",
            "sender@example.com",
            "recipient@example.com",
            "2024-01-15T10:00:00+00:00",
        )
        .with_labels(&["INBOX"]);

        let uid = mailbox.add_message(msg);
        assert_eq!(mailbox.message_count(), 1);

        // Star it
        mailbox.add_label(uid, "Starred");
        let msg = mailbox.get_message(uid).unwrap();
        assert!(msg.labels.contains("Starred"));

        // Move to Purgatory
        mailbox.move_message(uid, "INBOX", "Purgatory");
        let msg = mailbox.get_message(uid).unwrap();
        assert!(!msg.labels.contains("INBOX"));
        assert!(msg.labels.contains("Purgatory"));

        // Check move history
        let history = mailbox.get_move_history();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].to_label, "Purgatory");

        // Delete
        mailbox.delete_message(uid);
        assert_eq!(mailbox.message_count(), 0); // Deleted messages don't count

        // Expunge
        let expunged = mailbox.expunge();
        assert_eq!(expunged, vec![uid]);
        assert!(mailbox.get_message(uid).is_none());
    }

    #[test]
    fn test_mailbox_thread_simulation() {
        let mut mailbox = VirtualMailbox::new();

        // Simulate a 3-message thread
        let msg1 = MailboxMessage::new(
            0,
            "Original",
            "alice@test.com",
            "bob@test.com",
            "2024-01-15T10:00:00+00:00",
        )
        .with_labels(&["INBOX"])
        .with_message_id("<msg1@test.com>")
        .with_thread_id("gmail-thread-001");

        let msg2 = MailboxMessage::new(
            0,
            "Re: Original",
            "bob@test.com",
            "alice@test.com",
            "2024-01-15T11:00:00+00:00",
        )
        .with_labels(&["INBOX"])
        .with_message_id("<msg2@test.com>")
        .with_in_reply_to("<msg1@test.com>")
        .with_references(&["<msg1@test.com>"])
        .with_thread_id("gmail-thread-001");

        let msg3 = MailboxMessage::new(
            0,
            "Re: Re: Original",
            "alice@test.com",
            "bob@test.com",
            "2024-01-15T12:00:00+00:00",
        )
        .with_labels(&["INBOX", "Starred"])
        .with_message_id("<msg3@test.com>")
        .with_in_reply_to("<msg2@test.com>")
        .with_references(&["<msg1@test.com>", "<msg2@test.com>"])
        .with_thread_id("gmail-thread-001");

        let uid1 = mailbox.add_message(msg1);
        let uid2 = mailbox.add_message(msg2);
        let uid3 = mailbox.add_message(msg3);

        // All should be in the same thread
        assert_eq!(
            mailbox.get_message(uid1).unwrap().thread_id,
            mailbox.get_message(uid2).unwrap().thread_id
        );
        assert_eq!(
            mailbox.get_message(uid2).unwrap().thread_id,
            mailbox.get_message(uid3).unwrap().thread_id
        );

        // One message is starred
        let starred: Vec<_> = mailbox
            .get_all_messages()
            .into_iter()
            .filter(|m| m.labels.contains("Starred"))
            .collect();
        assert_eq!(starred.len(), 1);
        assert_eq!(starred[0].uid, uid3);
    }

    // ===== MockIMAPClient integration tests =====

    #[test]
    fn test_mock_client_full_workflow() {
        use std::sync::{Arc, RwLock};

        // Setup: Create mailbox, clock, and client
        let mailbox = Arc::new(RwLock::new(VirtualMailbox::new()));
        let clock = VirtualClock::at(
            chrono::DateTime::parse_from_rfc3339("2024-01-15T10:00:00+00:00")
                .unwrap()
                .with_timezone(&Utc),
        );
        let mut client = MockIMAPClient::new(Arc::clone(&mailbox), clock.clone());

        // Add test messages to mailbox
        let msg1 = MailboxMessage::new(
            0,
            "Important Email",
            "boss@company.com",
            "me@company.com",
            "2024-01-15T10:00:00+00:00",
        )
        .with_labels(&["INBOX"]);

        let msg2 = MailboxMessage::new(
            0,
            "Newsletter",
            "newsletter@spam.com",
            "me@company.com",
            "2024-01-10T10:00:00+00:00",
        )
        .with_labels(&["INBOX"]);

        let uid1 = mailbox.write().unwrap().add_message(msg1);
        let uid2 = mailbox.write().unwrap().add_message(msg2);

        // Simulate filter actions
        client.uid_store_add_flags(uid1, "\\Starred").unwrap();
        client.uid_move(uid2, "Purgatory").unwrap();

        // Verify actions were recorded
        let actions = client.get_recorded_actions();
        assert!(actions.iter().any(|a| a.is_star_for(uid1)));
        assert!(actions.iter().any(|a| a.is_move_to("Purgatory")));

        // Verify mailbox state changed
        let mb = mailbox.read().unwrap();
        assert!(mb.get_message(uid1).unwrap().labels.contains("\\Starred"));
        assert!(mb.get_message(uid2).unwrap().labels.contains("Purgatory"));
        assert!(!mb.get_message(uid2).unwrap().labels.contains("INBOX"));
    }

    #[test]
    fn test_mock_client_with_time_advancement() {
        use std::sync::{Arc, RwLock};

        let mailbox = Arc::new(RwLock::new(VirtualMailbox::new()));
        let clock = VirtualClock::at(
            chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00+00:00")
                .unwrap()
                .with_timezone(&Utc),
        );
        let client = MockIMAPClient::new(Arc::clone(&mailbox), clock.clone());

        // Initial time
        let t0 = client.now();
        assert_eq!(
            t0,
            chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00+00:00")
                .unwrap()
                .with_timezone(&Utc)
        );

        // Advance time by 7 days (simulating TTL check)
        clock.advance_days(7);

        let t1 = client.now();
        assert_eq!(
            t1,
            chrono::DateTime::parse_from_rfc3339("2024-01-08T00:00:00+00:00")
                .unwrap()
                .with_timezone(&Utc)
        );

        // Advance by another 3 days
        clock.advance_days(3);

        let t2 = client.now();
        assert_eq!(
            t2,
            chrono::DateTime::parse_from_rfc3339("2024-01-11T00:00:00+00:00")
                .unwrap()
                .with_timezone(&Utc)
        );
    }

    #[test]
    fn test_mock_client_purgatory_flow_simulation() {
        use std::sync::{Arc, RwLock};

        let mailbox = Arc::new(RwLock::new(VirtualMailbox::new()));
        let clock = VirtualClock::new();
        let mut client = MockIMAPClient::new(Arc::clone(&mailbox), clock);

        // Add a message
        let msg = MailboxMessage::new(
            0,
            "Old Email",
            "sender@example.com",
            "me@example.com",
            "2024-01-01T10:00:00+00:00",
        )
        .with_labels(&["INBOX"]);

        let uid = mailbox.write().unwrap().add_message(msg);

        // Step 1: Message ages and moves to Purgatory
        client.uid_move(uid, "Purgatory").unwrap();

        assert_eq!(client.get_move_actions().len(), 1);
        assert!(client.get_move_actions()[0].is_move_to("Purgatory"));

        // Clear actions for next step
        client.clear_recorded_actions();

        // Step 2: Message ages in Purgatory and moves to Oblivion
        client.select("Purgatory").unwrap();
        client.uid_move(uid, "Oblivion").unwrap();

        let move_actions = client.get_move_actions();
        assert_eq!(move_actions.len(), 1);
        match &move_actions[0] {
            RecordedAction::Move { from, to, .. } => {
                assert_eq!(from, "Purgatory");
                assert_eq!(to, "Oblivion");
            }
            _ => panic!("Expected Move action"),
        }

        // Verify final state
        let mb = mailbox.read().unwrap();
        let final_msg = mb.get_message(uid).unwrap();
        assert!(final_msg.labels.contains("Oblivion"));
        assert!(!final_msg.labels.contains("Purgatory"));
        assert!(!final_msg.labels.contains("INBOX"));
    }
}
