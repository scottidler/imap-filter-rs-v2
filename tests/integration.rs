// tests/integration.rs
//
// Integration test entry point for imap-filter-rs-v2.
// This file enables the test harness modules to be compiled and tested.

mod harness;

// Re-export for use in other test files
// TODO: Phase 2+ - These will be used by integration tests once MockIMAPClient is implemented
#[allow(unused_imports)] // TEMPORARY: Will be used in Phase 2 when integration tests are added
use harness::{
    Clock, EmailFixture, FixtureLoader, MailboxMessage, MoveRecord, RealClock, VirtualClock, VirtualMailbox,
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
}
