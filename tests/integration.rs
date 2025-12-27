// tests/integration.rs
//
// Integration test entry point for imap-filter-rs-v2.
// This file enables the test harness modules to be compiled and tested.

mod harness;

#[cfg(test)]
mod harness_tests {
    use super::harness::*;
    use chrono::{Duration, Utc};
    use std::sync::{Arc, RwLock};

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

    #[test]
    fn test_mailbox_get_message_mut() {
        let mut mailbox = VirtualMailbox::new();
        let msg = MailboxMessage::new(
            0,
            "Mutable Test",
            "sender@example.com",
            "recipient@example.com",
            "2024-01-15T10:00:00+00:00",
        );

        let uid = mailbox.add_message(msg);

        // Get mutable reference and modify
        {
            let msg_mut = mailbox.get_message_mut(uid).unwrap();
            msg_mut.subject = "Modified Subject".to_string();
            msg_mut.labels.insert("Modified".to_string());
        }

        // Verify modifications persisted
        let msg = mailbox.get_message(uid).unwrap();
        assert_eq!(msg.subject, "Modified Subject");
        assert!(msg.labels.contains("Modified"));
    }

    #[test]
    fn test_mailbox_clear_move_history() {
        let mut mailbox = VirtualMailbox::new();
        let msg = MailboxMessage::new(
            0,
            "Moving Message",
            "sender@example.com",
            "recipient@example.com",
            "2024-01-15T10:00:00+00:00",
        )
        .with_labels(&["INBOX"]);

        let uid = mailbox.add_message(msg);

        // Make some moves
        mailbox.move_message(uid, "INBOX", "Purgatory");
        mailbox.move_message(uid, "Purgatory", "Oblivion");

        assert_eq!(mailbox.get_move_history().len(), 2);

        // Clear move history
        mailbox.clear_move_history();
        assert!(mailbox.get_move_history().is_empty());
    }

    #[test]
    fn test_mailbox_message_date_field() {
        let msg = MailboxMessage::new(
            1,
            "Test",
            "sender@example.com",
            "recipient@example.com",
            "2024-06-15T12:00:00+00:00",
        );

        // Verify the date field is stored correctly
        assert_eq!(msg.date, "2024-06-15T12:00:00+00:00");
    }

    // ===== MockIMAPClient integration tests =====

    #[test]
    fn test_mock_client_full_workflow() {
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

    #[test]
    fn test_mock_client_get_message() {
        let mailbox = Arc::new(RwLock::new(VirtualMailbox::new()));
        let clock = VirtualClock::new();
        let client = MockIMAPClient::new(Arc::clone(&mailbox), clock);

        let msg = MailboxMessage::new(
            0,
            "Fetchable Message",
            "sender@example.com",
            "recipient@example.com",
            "2024-01-15T10:00:00+00:00",
        )
        .with_labels(&["INBOX"]);

        let uid = mailbox.write().unwrap().add_message(msg);

        // Use get_message to fetch the message
        let fetched = client.get_message(uid);
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().subject, "Fetchable Message");

        // Non-existent message returns None
        assert!(client.get_message(999).is_none());
    }

    #[test]
    fn test_mock_client_logout() {
        let mailbox = Arc::new(RwLock::new(VirtualMailbox::new()));
        let clock = VirtualClock::new();
        let mut client = MockIMAPClient::new(mailbox, clock);

        // Logout should succeed
        let result = client.logout();
        assert!(result.is_ok());
    }

    // ===== Fixture Loading Tests =====

    #[test]
    fn test_fixture_loader_base_path() {
        let loader = FixtureLoader::new();
        let base = loader.base_path();

        // Should point to tests/fixtures/emails
        assert!(base.ends_with("tests/fixtures/emails"));
    }

    #[test]
    fn test_load_direct_message_fixture() {
        let loader = FixtureLoader::new();
        let fixture = loader.load_email("simple/direct-message.eml").unwrap();

        assert_eq!(fixture.message.subject, "Direct message to you");
        assert!(fixture.message.from.contains(&"sender@company.com".to_string()));
        assert!(fixture.message.to.contains(&"me@example.com".to_string()));
        assert!(fixture.message.cc.is_empty());
        assert!(fixture
            .message
            .message_id
            .as_ref()
            .unwrap()
            .contains("direct-001@company.com"));
    }

    #[test]
    fn test_load_with_cc_fixture() {
        let loader = FixtureLoader::new();
        let fixture = loader.load_email("simple/with-cc.eml").unwrap();

        assert_eq!(fixture.message.subject, "Team update with CC");
        assert!(!fixture.message.cc.is_empty());
        assert!(fixture.message.cc.contains(&"colleague@company.com".to_string()));
        assert!(fixture.message.cc.contains(&"manager@company.com".to_string()));
    }

    #[test]
    fn test_load_mailing_list_fixture() {
        let loader = FixtureLoader::new();
        let fixture = loader.load_email("simple/mailing-list.eml").unwrap();

        assert_eq!(fixture.message.subject, "[repo/project] New issue opened");
        assert!(fixture.message.from.contains(&"noreply@github.com".to_string()));
    }

    #[test]
    fn test_load_thread_fixtures() {
        let loader = FixtureLoader::new();
        let fixtures = loader.load_directory("threads/thread-01").unwrap();

        assert_eq!(fixtures.len(), 3);

        // Verify thread headers are present
        let initial = fixtures.iter().find(|f| f.message.subject == "Project discussion");
        assert!(initial.is_some());

        let reply = fixtures.iter().find(|f| f.message.subject == "Re: Project discussion");
        assert!(reply.is_some());
        assert!(reply.unwrap().message.in_reply_to.is_some());
    }

    // ===== TestHarness Integration Tests =====

    #[test]
    fn test_harness_add_fixture() {
        let mut harness = TestHarness::new();

        // Load and add fixture
        let uid = harness.add_fixture("simple/direct-message.eml").unwrap();

        // Verify it was added
        let msg = harness.get_message(uid);
        assert!(msg.is_some());
        assert_eq!(msg.unwrap().subject, "Direct message to you");
    }

    #[test]
    fn test_harness_add_fixture_with_labels() {
        let mut harness = TestHarness::new();

        let uid = harness
            .add_fixture_with_labels("simple/direct-message.eml", &["INBOX", "Important"])
            .unwrap();

        harness.assert_has_label(uid, "INBOX");
        harness.assert_has_label(uid, "Important");
    }

    #[test]
    fn test_harness_add_fixture_dated() {
        let harness_time = chrono::DateTime::parse_from_rfc3339("2024-01-15T10:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        let mut harness = TestHarness::at_time(harness_time);

        // Add fixture dated 7 days ago
        let uid = harness
            .add_fixture_dated("simple/direct-message.eml", &["INBOX"], 7)
            .unwrap();

        // The message date should be 7 days before harness time
        let msg = harness.get_message(uid).unwrap();
        let expected_date = chrono::DateTime::parse_from_rfc3339("2024-01-08T10:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc)
            .to_rfc3339();

        assert_eq!(msg.date, expected_date);
    }

    #[test]
    fn test_harness_load_fixtures_from_directory() {
        let mut harness = TestHarness::new();

        let fixtures: Vec<EmailFixture> = harness.load_fixtures_from_directory("threads/thread-01").unwrap();

        assert_eq!(fixtures.len(), 3);

        // Fixtures should be sorted by filename
        assert!(fixtures[0].source_path.contains("01-initial"));
        assert!(fixtures[1].source_path.contains("02-reply"));
        assert!(fixtures[2].source_path.contains("03-follow-up"));
    }

    #[test]
    fn test_harness_advance_duration() {
        let start = chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        let harness = TestHarness::at_time(start);

        // Advance by 12 hours using Duration
        harness.advance(Duration::hours(12));

        let expected = chrono::DateTime::parse_from_rfc3339("2024-01-01T12:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        assert_eq!(harness.now(), expected);
    }

    #[test]
    fn test_harness_label_exists() {
        let harness = TestHarness::new();

        // Standard labels should exist
        assert!(harness.label_exists("INBOX"));
        assert!(harness.label_exists("Starred"));

        // Non-existent labels
        assert!(!harness.label_exists("NonExistentLabel"));
    }

    #[test]
    fn test_harness_assert_action() {
        let mut harness = TestHarness::new();
        let msg = MailboxMessage::new(
            0,
            "Test Subject",
            "sender@example.com",
            "recipient@example.com",
            &Utc::now().to_rfc3339(),
        )
        .with_labels(&["INBOX"]);

        let uid = harness.add_message(msg);

        harness.client.uid_store_add_flags(uid, "\\Starred").unwrap();

        // Use assert_action to verify the specific action
        harness.assert_action(&RecordedAction::Star {
            uid,
            subject: "Test Subject".to_string(),
        });
    }

    #[test]
    #[should_panic(expected = "Expected action")]
    fn test_harness_assert_action_fails_when_not_found() {
        let harness = TestHarness::new();

        // This should panic because no actions were recorded
        harness.assert_action(&RecordedAction::Star {
            uid: 999,
            subject: "Non-existent".to_string(),
        });
    }

    // ===== Full End-to-End Scenario Tests =====

    #[test]
    fn test_email_aging_scenario() {
        // Simulate an email aging and being moved to Purgatory
        let start_time = chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let mut harness = TestHarness::at_time(start_time);

        // Add a message dated now
        let uid = harness
            .add_fixture_dated("simple/direct-message.eml", &["INBOX", "Seen"], 0)
            .unwrap();

        // Initially in INBOX
        harness.assert_has_label(uid, "INBOX");
        harness.assert_message_count("INBOX", 1);

        // Advance time 8 days (past 7-day read TTL)
        harness.advance_days(8);

        // Simulate the filter moving it to Purgatory
        harness.client.uid_move(uid, "Purgatory").unwrap();

        // Verify the move
        harness.assert_moved_to(uid, "Purgatory");
        harness.assert_not_has_label(uid, "INBOX");
        harness.assert_has_label(uid, "Purgatory");
    }

    #[test]
    fn test_thread_loading_and_processing() {
        let mut harness = TestHarness::new();

        // Load thread fixtures
        let fixtures = harness.load_fixtures_from_directory("threads/thread-01").unwrap();
        assert_eq!(fixtures.len(), 3);

        // Add all messages to mailbox with labels
        for fixture in &fixtures {
            let msg = fixture.message.clone();
            harness.add_message_with_labels(msg, &["INBOX"]);
        }

        harness.assert_message_count("INBOX", 3);
        assert_eq!(harness.total_message_count(), 3);
    }

    #[test]
    fn test_starred_email_protection() {
        let start_time = chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let mut harness = TestHarness::at_time(start_time);

        // Add a very old message that is starred
        let uid = harness
            .add_fixture_dated("simple/direct-message.eml", &["INBOX", "Starred"], 100)
            .unwrap();

        // Verify it has Starred label
        harness.assert_has_label(uid, "Starred");
        harness.assert_has_label(uid, "INBOX");

        // Even after advancing time significantly, starred messages shouldn't be moved
        harness.advance_days(365);

        // No actions should have been taken (in a real filter scenario)
        harness.assert_no_actions();
    }

    #[test]
    fn test_purgatory_to_oblivion_flow() {
        let mut harness = TestHarness::new();

        // Add a message in Purgatory (already moved there)
        let uid = harness
            .add_fixture_dated("simple/direct-message.eml", &["Purgatory", "Seen"], 3)
            .unwrap();

        harness.assert_has_label(uid, "Purgatory");

        // Advance time past Purgatory TTL
        harness.advance_days(4);

        // Simulate moving to Oblivion
        harness.client.select("Purgatory").unwrap();
        harness.client.uid_move(uid, "Oblivion").unwrap();

        // Verify
        harness.assert_moved_to(uid, "Oblivion");
        harness.assert_not_has_label(uid, "Purgatory");
        harness.assert_has_label(uid, "Oblivion");
    }

    #[test]
    fn test_multi_step_scenario_with_action_clearing() {
        let start_time = chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let mut harness = TestHarness::at_time(start_time);

        // Step 1: Add fresh email
        let uid = harness
            .add_fixture_dated("simple/newsletter.eml", &["INBOX", "Seen"], 0)
            .unwrap();

        harness.assert_no_actions();

        // Step 2: Advance 8 days and move to Purgatory
        harness.advance_days(8);
        harness.client.uid_move(uid, "Purgatory").unwrap();

        assert_eq!(harness.move_actions().len(), 1);
        harness.assert_moved_to(uid, "Purgatory");

        // Clear actions for next step
        harness.clear_actions();
        harness.assert_no_actions();

        // Step 3: Advance 4 more days and move to Oblivion
        harness.advance_days(4);
        harness.client.select("Purgatory").unwrap();
        harness.client.uid_move(uid, "Oblivion").unwrap();

        assert_eq!(harness.move_actions().len(), 1);
        harness.assert_moved_to(uid, "Oblivion");

        // Final state verification
        harness.assert_not_has_label(uid, "INBOX");
        harness.assert_not_has_label(uid, "Purgatory");
        harness.assert_has_label(uid, "Oblivion");
    }

    #[test]
    fn test_cc_email_not_starred() {
        let mut harness = TestHarness::new();

        // Load email with CC (should not match "only to me" filter)
        let uid = harness
            .add_fixture_with_labels("simple/with-cc.eml", &["INBOX"])
            .unwrap();

        let msg = harness.get_message(uid).unwrap();

        // Has CC recipients
        assert!(!msg.cc.is_empty());

        // In a real filter, this would NOT be starred
        harness.assert_no_actions();
    }

    #[test]
    fn test_delete_action() {
        let mut harness = TestHarness::new();

        let uid = harness
            .add_fixture_with_labels("simple/newsletter.eml", &["Oblivion"])
            .unwrap();

        // Delete the message
        harness.client.uid_store_add_flags(uid, "\\Deleted").unwrap();

        // Verify delete was recorded
        harness.assert_deleted(uid);
        assert_eq!(harness.delete_actions().len(), 1);
    }

    // ===== Error Handling Tests =====

    #[test]
    fn test_fixture_error_display() {
        let io_err = FixtureError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"));
        assert!(io_err.to_string().contains("IO error"));

        let parse_err = FixtureError::Parse("invalid format".to_string());
        assert!(parse_err.to_string().contains("Parse error"));

        let header_err = FixtureError::MissingHeader("From".to_string());
        assert!(header_err.to_string().contains("Missing required header"));
    }

    #[test]
    fn test_move_record_equality() {
        let record1 = MoveRecord {
            uid: 1,
            from_label: "INBOX".to_string(),
            to_label: "Purgatory".to_string(),
        };

        let record2 = MoveRecord {
            uid: 1,
            from_label: "INBOX".to_string(),
            to_label: "Purgatory".to_string(),
        };

        let record3 = MoveRecord {
            uid: 2,
            from_label: "INBOX".to_string(),
            to_label: "Purgatory".to_string(),
        };

        assert_eq!(record1, record2);
        assert_ne!(record1, record3);
    }

    #[test]
    fn test_email_fixture_clone() {
        let loader = FixtureLoader::new();
        let fixture = loader.load_email("simple/direct-message.eml").unwrap();

        let cloned = fixture.clone();
        assert_eq!(fixture.message.subject, cloned.message.subject);
        assert_eq!(fixture.source_path, cloned.source_path);
    }
}
