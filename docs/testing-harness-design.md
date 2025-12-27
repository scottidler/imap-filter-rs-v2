# imap-filter-rs-v2: Integration Testing Harness Design

## Executive Summary

This document describes a comprehensive integration testing harness for `imap-filter-rs-v2` that enables:

1. **File-based email fixtures**: Load real/synthetic emails from `.eml` files stored in the repository
2. **Virtual time control**: Manipulate time to test TTL-based state transitions
3. **Full pipeline testing**: Test the complete email handling flow from ingestion to action execution
4. **Thread lifecycle testing**: Test email threads across multiple state transitions

---

## Goals & Non-Goals

### Goals

- Test message filters against realistic email content without network access
- Test state filter TTL expiry by controlling time
- Verify thread-aware behavior (thread protection, newest-message TTL)
- Capture and assert on all IMAP actions (star, flag, move, delete)
- Enable regression testing for filter logic
- Support complex multi-step scenarios (email arrives → ages → moves to purgatory → ages → deleted)

### Non-Goals

- Testing actual IMAP protocol compliance (that's the `imap` crate's job)
- Performance/load testing
- OAuth2 token refresh testing (separate concern)

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         Integration Test Harness                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  ┌──────────────────┐    ┌──────────────────┐    ┌─────────────────────┐   │
│  │ EmailFixtures    │───▶│ VirtualMailbox   │───▶│ TestIMAPFilter      │   │
│  │ (load .eml files)│    │ (in-memory state)│    │ (uses MockClient)   │   │
│  └──────────────────┘    └──────────────────┘    └─────────────────────┘   │
│           │                      │                        │                 │
│           │                      │                        ▼                 │
│           │                      │              ┌─────────────────────┐    │
│           │                      │              │ MockIMAPClient      │    │
│           ▼                      ▼              │ - records actions   │    │
│  ┌──────────────────┐    ┌──────────────────┐  │ - returns test data │    │
│  │ test_emails/     │    │ VirtualClock     │  └─────────────────────┘    │
│  │ ├── simple/      │    │ (controllable    │            │                 │
│  │ ├── threads/     │    │  Instant)        │            ▼                 │
│  │ └── edge_cases/  │    └──────────────────┘  ┌─────────────────────┐    │
│  └──────────────────┘                          │ ActionRecorder      │    │
│                                                │ - Star, Flag, Move  │    │
│                                                │ - assertion helpers │    │
│                                                └─────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Component Design

### 1. Email Fixtures System

#### Directory Structure

```
tests/
├── fixtures/
│   ├── emails/
│   │   ├── simple/
│   │   │   ├── direct-message.eml      # Simple To: me email
│   │   │   ├── with-cc.eml             # Email with CC recipients
│   │   │   ├── mailing-list.eml        # Email with List-Id header
│   │   │   └── high-priority.eml       # Email with X-Priority: 1
│   │   ├── threads/
│   │   │   ├── thread-01/
│   │   │   │   ├── 01-initial.eml      # Root message
│   │   │   │   ├── 02-reply.eml        # Reply with In-Reply-To
│   │   │   │   └── 03-reply-all.eml    # Reply-all with References
│   │   │   └── thread-02/
│   │   │       └── ...
│   │   ├── edge-cases/
│   │   │   ├── no-message-id.eml
│   │   │   ├── malformed-headers.eml
│   │   │   └── unicode-subject.eml
│   │   └── scenarios/
│   │       └── purgatory-flow/
│   │           ├── manifest.yaml       # Scenario definition
│   │           └── emails/
│   │               └── ...
│   └── configs/
│       ├── basic-filters.yml
│       ├── state-transitions.yml
│       └── thread-protection.yml
├── harness/
│   ├── mod.rs
│   ├── fixtures.rs
│   ├── virtual_clock.rs
│   ├── virtual_mailbox.rs
│   ├── mock_client.rs
│   └── action_recorder.rs
└── integration/
    ├── message_filter_tests.rs
    ├── state_filter_tests.rs
    ├── thread_tests.rs
    └── scenario_tests.rs
```

#### Fixture Loader

```rust
// tests/harness/fixtures.rs

use std::path::Path;
use crate::message::Message;
use chrono::{DateTime, Utc};

/// Represents a loaded email fixture with metadata
#[derive(Debug, Clone)]
pub struct EmailFixture {
    /// The parsed Message struct
    pub message: Message,
    /// Original file path for debugging
    pub source_path: String,
    /// Override for internal date (for TTL testing)
    pub internal_date_override: Option<DateTime<Utc>>,
}

/// Loader for email fixtures from .eml files
pub struct FixtureLoader {
    base_path: PathBuf,
}

impl FixtureLoader {
    pub fn new() -> Self {
        Self {
            base_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("emails"),
        }
    }

    /// Load a single .eml file into a Message
    pub fn load_email(&self, relative_path: &str) -> Result<EmailFixture> {
        let path = self.base_path.join(relative_path);
        let content = std::fs::read(&path)?;

        // Parse the .eml file (full RFC 822 message)
        let parsed = mailparse::parse_mail(&content)?;

        // Extract headers
        let headers: Vec<u8> = extract_headers(&parsed);

        // Build Message using existing constructor
        // Default UID assigned sequentially, can be overridden
        let message = Message::new(
            0,    // UID - set later by VirtualMailbox
            0,    // seq - set later
            headers,
            vec![], // labels - set later
            Utc::now().to_rfc3339(), // date - can be overridden
            None,  // gmail_thread_id - for non-Gmail testing
        );

        Ok(EmailFixture {
            message,
            source_path: path.to_string_lossy().to_string(),
            internal_date_override: None,
        })
    }

    /// Load all emails from a directory (for thread testing)
    pub fn load_directory(&self, relative_path: &str) -> Result<Vec<EmailFixture>> {
        let dir = self.base_path.join(relative_path);
        let mut fixtures = Vec::new();

        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "eml").unwrap_or(false) {
                let relative = path.strip_prefix(&self.base_path)?
                    .to_string_lossy()
                    .to_string();
                fixtures.push(self.load_email(&relative)?);
            }
        }

        // Sort by filename for predictable ordering
        fixtures.sort_by(|a, b| a.source_path.cmp(&b.source_path));
        Ok(fixtures)
    }

    /// Load a test scenario from a manifest
    pub fn load_scenario(&self, scenario_name: &str) -> Result<TestScenario> {
        let manifest_path = self.base_path
            .join("scenarios")
            .join(scenario_name)
            .join("manifest.yaml");

        let content = std::fs::read_to_string(&manifest_path)?;
        let scenario: TestScenario = serde_yaml::from_str(&content)?;

        Ok(scenario)
    }
}
```

### 2. Virtual Clock

The virtual clock allows tests to control time, enabling testing of TTL-based state transitions.

```rust
// tests/harness/virtual_clock.rs

use chrono::{DateTime, Duration, Utc};
use std::sync::{Arc, RwLock};

/// A clock that can be controlled for testing
#[derive(Clone)]
pub struct VirtualClock {
    inner: Arc<RwLock<DateTime<Utc>>>,
}

impl VirtualClock {
    /// Create a new virtual clock set to the current time
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Utc::now())),
        }
    }

    /// Create a virtual clock set to a specific time
    pub fn at(time: DateTime<Utc>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(time)),
        }
    }

    /// Get the current virtual time
    pub fn now(&self) -> DateTime<Utc> {
        *self.inner.read().unwrap()
    }

    /// Advance time by the given duration
    pub fn advance(&self, duration: Duration) {
        let mut guard = self.inner.write().unwrap();
        *guard = *guard + duration;
    }

    /// Advance time by the given number of days
    pub fn advance_days(&self, days: i64) {
        self.advance(Duration::days(days));
    }

    /// Set the clock to a specific time
    pub fn set(&self, time: DateTime<Utc>) {
        *self.inner.write().unwrap() = time;
    }

    /// Rewind time by the given duration
    pub fn rewind(&self, duration: Duration) {
        let mut guard = self.inner.write().unwrap();
        *guard = *guard - duration;
    }
}

impl Default for VirtualClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for components that need to access time
/// This allows production code to use real time while tests use virtual time
pub trait Clock: Clone + Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

impl Clock for VirtualClock {
    fn now(&self) -> DateTime<Utc> {
        self.now()
    }
}

/// Real clock for production use
#[derive(Clone, Default)]
pub struct RealClock;

impl Clock for RealClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}
```

### 3. Virtual Mailbox

The virtual mailbox holds the in-memory state of emails, simulating an IMAP mailbox.

```rust
// tests/harness/virtual_mailbox.rs

use crate::message::Message;
use crate::cfg::label::Label;
use std::collections::{HashMap, HashSet};

/// Represents the state of a message in the virtual mailbox
#[derive(Debug, Clone)]
pub struct MailboxMessage {
    pub message: Message,
    pub labels: HashSet<String>,
    pub flags: HashSet<String>,
    pub deleted: bool,
}

/// In-memory IMAP mailbox for testing
#[derive(Debug, Default)]
pub struct VirtualMailbox {
    /// Messages by UID
    messages: HashMap<u32, MailboxMessage>,
    /// Next UID to assign
    next_uid: u32,
    /// All labels/folders that exist
    labels: HashSet<String>,
    /// Messages that have been moved (from_uid -> to_label)
    moves: Vec<MoveRecord>,
}

#[derive(Debug, Clone)]
pub struct MoveRecord {
    pub uid: u32,
    pub from_label: String,
    pub to_label: String,
}

impl VirtualMailbox {
    pub fn new() -> Self {
        let mut labels = HashSet::new();
        labels.insert("INBOX".to_string());
        labels.insert("\\Starred".to_string());
        labels.insert("\\Important".to_string());

        Self {
            messages: HashMap::new(),
            next_uid: 1,
            labels,
            moves: Vec::new(),
        }
    }

    /// Add a message to the mailbox
    pub fn add_message(&mut self, mut message: Message) -> u32 {
        let uid = self.next_uid;
        self.next_uid += 1;

        message.uid = uid;
        message.seq = uid; // seq matches uid for simplicity

        let labels: HashSet<String> = message.labels
            .iter()
            .map(|l| l.to_string())
            .collect();

        self.messages.insert(uid, MailboxMessage {
            message,
            labels,
            flags: HashSet::new(),
            deleted: false,
        });

        uid
    }

    /// Add a message with specific labels
    pub fn add_message_with_labels(
        &mut self,
        mut message: Message,
        labels: Vec<&str>
    ) -> u32 {
        message.labels = labels.iter().map(|s| Label::new(s)).collect();
        self.add_message(message)
    }

    /// Get a message by UID
    pub fn get_message(&self, uid: u32) -> Option<&MailboxMessage> {
        self.messages.get(&uid)
    }

    /// Get all messages (non-deleted)
    pub fn get_all_messages(&self) -> Vec<Message> {
        self.messages
            .values()
            .filter(|m| !m.deleted)
            .map(|m| m.message.clone())
            .collect()
    }

    /// Get messages with a specific label
    pub fn get_messages_with_label(&self, label: &str) -> Vec<Message> {
        self.messages
            .values()
            .filter(|m| !m.deleted && m.labels.contains(label))
            .map(|m| m.message.clone())
            .collect()
    }

    /// Add a label to a message
    pub fn add_label(&mut self, uid: u32, label: &str) -> bool {
        if let Some(msg) = self.messages.get_mut(&uid) {
            msg.labels.insert(label.to_string());

            // Update the Message struct's labels too
            if !msg.message.labels.iter().any(|l| l.to_string() == label) {
                msg.message.labels.push(Label::new(label));
            }
            true
        } else {
            false
        }
    }

    /// Remove a label from a message
    pub fn remove_label(&mut self, uid: u32, label: &str) -> bool {
        if let Some(msg) = self.messages.get_mut(&uid) {
            msg.labels.remove(label);
            msg.message.labels.retain(|l| l.to_string() != label);
            true
        } else {
            false
        }
    }

    /// Move a message from one folder to another
    pub fn move_message(&mut self, uid: u32, from: &str, to: &str) -> bool {
        if let Some(msg) = self.messages.get_mut(&uid) {
            msg.labels.remove(from);
            msg.labels.insert(to.to_string());

            // Update Message struct
            msg.message.labels.retain(|l| l.to_string() != from);
            msg.message.labels.push(Label::new(to));

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

    /// Mark a message as deleted
    pub fn delete_message(&mut self, uid: u32) -> bool {
        if let Some(msg) = self.messages.get_mut(&uid) {
            msg.deleted = true;
            msg.flags.insert("\\Deleted".to_string());
            true
        } else {
            false
        }
    }

    /// Expunge deleted messages (actually remove them)
    pub fn expunge(&mut self) -> Vec<u32> {
        let deleted: Vec<u32> = self.messages
            .iter()
            .filter(|(_, m)| m.deleted)
            .map(|(uid, _)| *uid)
            .collect();

        for uid in &deleted {
            self.messages.remove(uid);
        }

        deleted
    }

    /// Get the move history for assertions
    pub fn get_move_history(&self) -> &[MoveRecord] {
        &self.moves
    }

    /// Check if a label/folder exists
    pub fn label_exists(&self, label: &str) -> bool {
        self.labels.contains(label)
    }

    /// Create a label
    pub fn create_label(&mut self, label: &str) {
        self.labels.insert(label.to_string());
    }
}
```

### 4. Mock IMAP Client

The mock client implements IMAP-like operations against the virtual mailbox.

```rust
// tests/harness/mock_client.rs

use crate::harness::{VirtualMailbox, ActionRecorder, VirtualClock};
use crate::message::Message;
use eyre::Result;
use std::sync::{Arc, RwLock};

/// Recorded action types for verification
#[derive(Debug, Clone, PartialEq)]
pub enum RecordedAction {
    Star { uid: u32, subject: String },
    Flag { uid: u32, subject: String },
    Move { uid: u32, from: String, to: String, subject: String },
    Delete { uid: u32, subject: String },
    AddLabel { uid: u32, label: String },
    RemoveLabel { uid: u32, label: String },
    CreateLabel { label: String },
}

/// Mock IMAP client for testing
pub struct MockIMAPClient {
    mailbox: Arc<RwLock<VirtualMailbox>>,
    actions: Arc<RwLock<Vec<RecordedAction>>>,
    current_folder: String,
    clock: VirtualClock,
}

impl MockIMAPClient {
    pub fn new(mailbox: Arc<RwLock<VirtualMailbox>>, clock: VirtualClock) -> Self {
        Self {
            mailbox,
            actions: Arc::new(RwLock::new(Vec::new())),
            current_folder: "INBOX".to_string(),
            clock,
        }
    }

    /// Select a mailbox/folder
    pub fn select(&mut self, folder: &str) -> Result<()> {
        self.current_folder = folder.to_string();
        Ok(())
    }

    /// Search for messages (simplified - returns all in current folder)
    pub fn search(&self, _query: &str) -> Result<Vec<u32>> {
        let mailbox = self.mailbox.read().unwrap();
        let uids: Vec<u32> = mailbox
            .get_messages_with_label(&self.current_folder)
            .iter()
            .map(|m| m.uid)
            .collect();
        Ok(uids)
    }

    /// Fetch messages by sequence set
    pub fn fetch_messages(&self) -> Result<Vec<Message>> {
        let mailbox = self.mailbox.read().unwrap();
        Ok(mailbox.get_messages_with_label(&self.current_folder))
    }

    /// Get labels for a message
    pub fn get_labels(&self, uid: u32) -> Result<Vec<String>> {
        let mailbox = self.mailbox.read().unwrap();
        if let Some(msg) = mailbox.get_message(uid) {
            Ok(msg.labels.iter().cloned().collect())
        } else {
            Ok(vec![])
        }
    }

    /// Add a flag/label to a message
    pub fn uid_store_add_flags(&mut self, uid: u32, flag: &str) -> Result<()> {
        let subject = {
            let mailbox = self.mailbox.read().unwrap();
            mailbox.get_message(uid)
                .map(|m| m.message.subject.clone())
                .unwrap_or_default()
        };

        let action = if flag == "\\Starred" {
            RecordedAction::Star { uid, subject }
        } else if flag == "\\Important" {
            RecordedAction::Flag { uid, subject }
        } else if flag == "\\Deleted" {
            let mut mailbox = self.mailbox.write().unwrap();
            mailbox.delete_message(uid);
            RecordedAction::Delete { uid, subject }
        } else {
            RecordedAction::AddLabel { uid, label: flag.to_string() }
        };

        let mut mailbox = self.mailbox.write().unwrap();
        mailbox.add_label(uid, flag);

        self.actions.write().unwrap().push(action);
        Ok(())
    }

    /// Move a message to another folder
    pub fn uid_move(&mut self, uid: u32, destination: &str) -> Result<()> {
        let subject = {
            let mailbox = self.mailbox.read().unwrap();
            mailbox.get_message(uid)
                .map(|m| m.message.subject.clone())
                .unwrap_or_default()
        };

        let action = RecordedAction::Move {
            uid,
            from: self.current_folder.clone(),
            to: destination.to_string(),
            subject,
        };

        let mut mailbox = self.mailbox.write().unwrap();
        mailbox.move_message(uid, &self.current_folder, destination);

        self.actions.write().unwrap().push(action);
        Ok(())
    }

    /// Create a label/folder
    pub fn create_label(&mut self, label: &str) -> Result<()> {
        let mut mailbox = self.mailbox.write().unwrap();
        mailbox.create_label(label);

        self.actions.write().unwrap().push(
            RecordedAction::CreateLabel { label: label.to_string() }
        );
        Ok(())
    }

    /// Check if a label exists
    pub fn label_exists(&self, label: &str) -> bool {
        let mailbox = self.mailbox.read().unwrap();
        mailbox.label_exists(label)
    }

    /// Get all recorded actions for assertions
    pub fn get_recorded_actions(&self) -> Vec<RecordedAction> {
        self.actions.read().unwrap().clone()
    }

    /// Clear recorded actions
    pub fn clear_recorded_actions(&self) {
        self.actions.write().unwrap().clear();
    }

    /// Get the current virtual time
    pub fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.clock.now()
    }

    /// Simulate logout
    pub fn logout(&mut self) -> Result<()> {
        Ok(())
    }
}
```

### 5. Refactoring Production Code for Testability

To make the production code testable, we need to introduce trait abstractions.

#### 5.1 IMAP Client Trait

```rust
// src/imap_client.rs (NEW FILE)

use crate::message::Message;
use eyre::Result;
use std::collections::HashSet;

/// Trait abstracting IMAP operations for testability
pub trait IMAPClientOps {
    /// Select a mailbox
    fn select_mailbox(&mut self, name: &str) -> Result<()>;

    /// Search for message UIDs
    fn search_all(&mut self) -> Result<Vec<u32>>;

    /// Fetch messages with full details
    fn fetch_messages(&mut self, uids: &[u32]) -> Result<Vec<Message>>;

    /// Get labels for a message
    fn get_labels(&mut self, uid: u32) -> Result<HashSet<String>>;

    /// Add a label/flag to a message
    fn add_label(&mut self, uid: u32, label: &str) -> Result<()>;

    /// Remove a label/flag from a message
    fn remove_label(&mut self, uid: u32, label: &str) -> Result<()>;

    /// Move a message to a destination folder
    fn move_message(&mut self, uid: u32, destination: &str) -> Result<()>;

    /// Delete a message (add \Deleted flag)
    fn delete_message(&mut self, uid: u32) -> Result<()>;

    /// Ensure a label exists, creating if necessary
    fn ensure_label(&mut self, label: &str) -> Result<()>;

    /// Logout from the session
    fn logout(&mut self) -> Result<()>;
}

/// Real IMAP client implementation using the `imap` crate
pub struct RealIMAPClient<T: std::io::Read + std::io::Write> {
    session: imap::Session<T>,
}

impl<T: std::io::Read + std::io::Write> RealIMAPClient<T> {
    pub fn new(session: imap::Session<T>) -> Self {
        Self { session }
    }
}

impl<T: std::io::Read + std::io::Write> IMAPClientOps for RealIMAPClient<T> {
    fn select_mailbox(&mut self, name: &str) -> Result<()> {
        self.session.select(name)?;
        Ok(())
    }

    fn search_all(&mut self) -> Result<Vec<u32>> {
        let seqs = self.session.search("ALL")?;
        Ok(seqs.into_iter().collect())
    }

    // ... implement remaining methods using existing utils.rs functions
}
```

#### 5.2 Clock Trait Integration

Modify `StateFilter::evaluate_ttl` to accept a generic clock:

```rust
// src/cfg/state_filter.rs - modified signature

impl StateFilter {
    /// Evaluate TTL with an injectable clock
    pub fn evaluate_ttl_with_clock<C: Clock>(
        &self,
        msg: &Message,
        clock: &C
    ) -> eyre::Result<Option<StateAction>> {
        let now = clock.now();
        // ... existing logic using `now`
    }

    /// Convenience method for production use
    pub fn evaluate_ttl(&self, msg: &Message) -> eyre::Result<Option<StateAction>> {
        self.evaluate_ttl_with_clock(msg, &RealClock)
    }
}
```

### 6. Test Scenario Manifest

For complex multi-step tests, we use YAML manifests:

```yaml
# tests/fixtures/emails/scenarios/purgatory-flow/manifest.yaml

name: purgatory-flow
description: |
  Test the complete lifecycle of an email through the purgatory workflow:
  1. Email arrives in INBOX
  2. Email ages past read TTL (7d)
  3. Email moves to Purgatory
  4. Email ages past purgatory TTL (3d)
  5. Email moves to Oblivion

config: state-transitions.yml

steps:
  - name: initial_state
    description: "Email arrives fresh in INBOX"
    load_emails:
      - path: emails/regular-email.eml
        labels: [INBOX, Seen]
        internal_date: now
    assert:
      - type: message_count
        label: INBOX
        count: 1
      - type: message_count
        label: Purgatory
        count: 0

  - name: after_7_days
    description: "Email has aged past read TTL"
    advance_time: 8d
    run_filter: true
    assert:
      - type: action_recorded
        action: Move
        to: Purgatory
      - type: message_count
        label: INBOX
        count: 0
      - type: message_count
        label: Purgatory
        count: 1

  - name: after_10_days
    description: "Email has aged in purgatory past purge TTL"
    advance_time: 3d
    run_filter: true
    assert:
      - type: action_recorded
        action: Move
        to: Oblivion
      - type: message_count
        label: Purgatory
        count: 0
      - type: message_count
        label: Oblivion
        count: 1
```

### 7. Test Harness API

```rust
// tests/harness/mod.rs

pub mod fixtures;
pub mod virtual_clock;
pub mod virtual_mailbox;
pub mod mock_client;

pub use fixtures::{EmailFixture, FixtureLoader};
pub use virtual_clock::{VirtualClock, Clock, RealClock};
pub use virtual_mailbox::{VirtualMailbox, MailboxMessage, MoveRecord};
pub use mock_client::{MockIMAPClient, RecordedAction};

use crate::cfg::config::Config;
use crate::cfg::message_filter::MessageFilter;
use crate::cfg::state_filter::StateFilter;
use std::sync::{Arc, RwLock};

/// High-level test harness combining all components
pub struct TestHarness {
    pub mailbox: Arc<RwLock<VirtualMailbox>>,
    pub clock: VirtualClock,
    pub client: MockIMAPClient,
    pub message_filters: Vec<MessageFilter>,
    pub state_filters: Vec<StateFilter>,
}

impl TestHarness {
    /// Create a new test harness with default configuration
    pub fn new() -> Self {
        let mailbox = Arc::new(RwLock::new(VirtualMailbox::new()));
        let clock = VirtualClock::new();
        let client = MockIMAPClient::new(Arc::clone(&mailbox), clock.clone());

        Self {
            mailbox,
            clock,
            client,
            message_filters: Vec::new(),
            state_filters: Vec::new(),
        }
    }

    /// Create from a config file path
    pub fn from_config(config_path: &str) -> eyre::Result<Self> {
        let content = std::fs::read_to_string(config_path)?;
        let config: Config = serde_yaml::from_str(&content)?;

        let mailbox = Arc::new(RwLock::new(VirtualMailbox::new()));
        let clock = VirtualClock::new();
        let client = MockIMAPClient::new(Arc::clone(&mailbox), clock.clone());

        Ok(Self {
            mailbox,
            clock,
            client,
            message_filters: config.message_filters,
            state_filters: config.state_filters,
        })
    }

    /// Load and add a fixture email to the mailbox
    pub fn add_fixture(&mut self, fixture_path: &str) -> eyre::Result<u32> {
        let loader = FixtureLoader::new();
        let fixture = loader.load_email(fixture_path)?;
        let uid = self.mailbox.write().unwrap().add_message(fixture.message);
        Ok(uid)
    }

    /// Add a fixture with specific labels
    pub fn add_fixture_with_labels(
        &mut self,
        fixture_path: &str,
        labels: Vec<&str>
    ) -> eyre::Result<u32> {
        let loader = FixtureLoader::new();
        let fixture = loader.load_email(fixture_path)?;
        let uid = self.mailbox.write().unwrap()
            .add_message_with_labels(fixture.message, labels);
        Ok(uid)
    }

    /// Add a fixture with a specific internal date (for TTL testing)
    pub fn add_fixture_dated(
        &mut self,
        fixture_path: &str,
        labels: Vec<&str>,
        days_ago: i64,
    ) -> eyre::Result<u32> {
        let loader = FixtureLoader::new();
        let mut fixture = loader.load_email(fixture_path)?;

        // Set the internal date to `days_ago` days before current virtual time
        let date = self.clock.now() - chrono::Duration::days(days_ago);
        fixture.message.date = date.to_rfc3339();

        let uid = self.mailbox.write().unwrap()
            .add_message_with_labels(fixture.message, labels);
        Ok(uid)
    }

    /// Advance virtual time
    pub fn advance_time(&self, days: i64) {
        self.clock.advance_days(days);
    }

    /// Run message filters on all messages
    pub fn run_message_filters(&mut self) -> eyre::Result<()> {
        // Implementation using mock client
        todo!("Implement using TestIMAPFilter")
    }

    /// Run state filters on all messages
    pub fn run_state_filters(&mut self) -> eyre::Result<()> {
        // Implementation using mock client with virtual clock
        todo!("Implement using TestIMAPFilter")
    }

    /// Run both filter phases
    pub fn run_all_filters(&mut self) -> eyre::Result<()> {
        self.run_message_filters()?;
        self.run_state_filters()
    }

    /// Get recorded actions for assertions
    pub fn actions(&self) -> Vec<RecordedAction> {
        self.client.get_recorded_actions()
    }

    /// Clear recorded actions
    pub fn clear_actions(&self) {
        self.client.clear_recorded_actions();
    }

    /// Assert that a specific action was recorded
    pub fn assert_action_recorded(&self, expected: &RecordedAction) {
        let actions = self.actions();
        assert!(
            actions.contains(expected),
            "Expected action {:?} not found in {:?}",
            expected, actions
        );
    }

    /// Assert no actions were recorded
    pub fn assert_no_actions(&self) {
        let actions = self.actions();
        assert!(
            actions.is_empty(),
            "Expected no actions but found {:?}",
            actions
        );
    }

    /// Get message count in a label/folder
    pub fn message_count(&self, label: &str) -> usize {
        self.mailbox.read().unwrap()
            .get_messages_with_label(label)
            .len()
    }
}
```

---

## Example Tests

### Basic Message Filter Test

```rust
// tests/integration/message_filter_tests.rs

use crate::harness::{TestHarness, RecordedAction};

#[test]
fn test_only_to_me_filter_stars_direct_email() {
    let mut harness = TestHarness::from_config(
        "tests/fixtures/configs/basic-filters.yml"
    ).unwrap();

    // Load a direct email (only to me, no CC)
    harness.add_fixture_with_labels(
        "simple/direct-message.eml",
        vec!["INBOX"]
    ).unwrap();

    // Run filters
    harness.run_message_filters().unwrap();

    // Assert the message was starred
    harness.assert_action_recorded(&RecordedAction::Star {
        uid: 1,
        subject: "Direct message to you".to_string(),
    });
}

#[test]
fn test_email_with_cc_not_starred() {
    let mut harness = TestHarness::from_config(
        "tests/fixtures/configs/basic-filters.yml"
    ).unwrap();

    // Load an email with CC recipients
    harness.add_fixture_with_labels(
        "simple/with-cc.eml",
        vec!["INBOX"]
    ).unwrap();

    // Run filters
    harness.run_message_filters().unwrap();

    // Assert no Star action (email has CC, doesn't match only-to-me)
    let actions = harness.actions();
    assert!(!actions.iter().any(|a| matches!(a, RecordedAction::Star { .. })));
}
```

### State Filter TTL Test

```rust
// tests/integration/state_filter_tests.rs

use crate::harness::{TestHarness, RecordedAction};

#[test]
fn test_read_email_expires_after_7_days() {
    let mut harness = TestHarness::from_config(
        "tests/fixtures/configs/state-transitions.yml"
    ).unwrap();

    // Add an email that arrived 8 days ago (past 7-day read TTL)
    harness.add_fixture_dated(
        "simple/direct-message.eml",
        vec!["INBOX", "Seen"],  // Seen = read
        8,  // 8 days ago
    ).unwrap();

    // Run state filters
    harness.run_state_filters().unwrap();

    // Assert the message was moved to Purgatory
    harness.assert_action_recorded(&RecordedAction::Move {
        uid: 1,
        from: "INBOX".to_string(),
        to: "Purgatory".to_string(),
        subject: "Direct message to you".to_string(),
    });
}

#[test]
fn test_unread_email_survives_7_days() {
    let mut harness = TestHarness::from_config(
        "tests/fixtures/configs/state-transitions.yml"
    ).unwrap();

    // Add an UNREAD email that arrived 8 days ago
    // (past read TTL of 7d but not past unread TTL of 21d)
    harness.add_fixture_dated(
        "simple/direct-message.eml",
        vec!["INBOX"],  // No Seen flag = unread
        8,  // 8 days ago
    ).unwrap();

    // Run state filters
    harness.run_state_filters().unwrap();

    // Assert NO move action - unread TTL is 21 days
    harness.assert_no_actions();
}

#[test]
fn test_starred_email_never_expires() {
    let mut harness = TestHarness::from_config(
        "tests/fixtures/configs/state-transitions.yml"
    ).unwrap();

    // Add a STARRED email from 100 days ago
    harness.add_fixture_dated(
        "simple/direct-message.eml",
        vec!["INBOX", "Seen", "Starred"],
        100,  // Very old
    ).unwrap();

    // Run state filters
    harness.run_state_filters().unwrap();

    // Assert no actions - starred emails are protected
    harness.assert_no_actions();
}
```

### Thread Protection Test

```rust
// tests/integration/thread_tests.rs

use crate::harness::{TestHarness, RecordedAction};

#[test]
fn test_thread_protected_by_starred_reply() {
    let mut harness = TestHarness::from_config(
        "tests/fixtures/configs/thread-protection.yml"
    ).unwrap();

    // Load a thread where:
    // - Original message is 10 days old (past TTL)
    // - Reply is starred (should protect entire thread)
    let loader = FixtureLoader::new();
    let fixtures = loader.load_directory("threads/thread-01").unwrap();

    // Add original message (old, would normally expire)
    let mut original = fixtures[0].message.clone();
    original.date = (harness.clock.now() - chrono::Duration::days(10)).to_rfc3339();
    original.labels = vec![Label::new("INBOX"), Label::new("Seen")];
    harness.mailbox.write().unwrap().add_message(original);

    // Add reply (starred - protects thread)
    let mut reply = fixtures[1].message.clone();
    reply.date = harness.clock.now().to_rfc3339();
    reply.labels = vec![Label::new("INBOX"), Label::new("Starred")];
    harness.mailbox.write().unwrap().add_message(reply);

    // Run state filters
    harness.run_state_filters().unwrap();

    // Assert no actions - starred reply protects entire thread
    harness.assert_no_actions();
}

#[test]
fn test_thread_newest_message_determines_ttl() {
    let mut harness = TestHarness::from_config(
        "tests/fixtures/configs/state-transitions.yml"
    ).unwrap();

    // Load a thread where:
    // - Original message is 10 days old
    // - Reply is 3 days old
    // Thread should NOT expire (newest message is only 3 days old)

    let loader = FixtureLoader::new();
    let fixtures = loader.load_directory("threads/thread-01").unwrap();

    // Add original message (old)
    let mut original = fixtures[0].message.clone();
    original.date = (harness.clock.now() - chrono::Duration::days(10)).to_rfc3339();
    original.labels = vec![Label::new("INBOX"), Label::new("Seen")];
    harness.mailbox.write().unwrap().add_message(original);

    // Add reply (recent)
    let mut reply = fixtures[1].message.clone();
    reply.date = (harness.clock.now() - chrono::Duration::days(3)).to_rfc3339();
    reply.labels = vec![Label::new("INBOX"), Label::new("Seen")];
    harness.mailbox.write().unwrap().add_message(reply);

    // Run state filters
    harness.run_state_filters().unwrap();

    // Assert no actions - newest message is only 3 days old
    harness.assert_no_actions();

    // Advance time by 5 more days (now newest is 8 days old)
    harness.advance_time(5);
    harness.clear_actions();
    harness.run_state_filters().unwrap();

    // Now both messages should be moved (thread expired)
    let actions = harness.actions();
    assert_eq!(actions.len(), 2);
    assert!(actions.iter().all(|a| matches!(a, RecordedAction::Move { to, .. } if to == "Purgatory")));
}
```

### Full Scenario Test

```rust
// tests/integration/scenario_tests.rs

use crate::harness::{TestHarness, FixtureLoader};

#[test]
fn test_purgatory_flow_scenario() {
    let loader = FixtureLoader::new();
    let scenario = loader.load_scenario("purgatory-flow").unwrap();

    let mut harness = TestHarness::from_config(&scenario.config_path).unwrap();

    for step in &scenario.steps {
        println!("Executing step: {}", step.name);

        // Load emails for this step
        for email in &step.load_emails {
            harness.add_fixture_dated(
                &email.path,
                email.labels.iter().map(|s| s.as_str()).collect(),
                email.days_ago.unwrap_or(0),
            ).unwrap();
        }

        // Advance time if specified
        if let Some(time_spec) = &step.advance_time {
            let days = parse_duration(time_spec);
            harness.advance_time(days);
        }

        // Run filters if specified
        if step.run_filter {
            harness.run_all_filters().unwrap();
        }

        // Run assertions
        for assertion in &step.assertions {
            match assertion {
                Assertion::MessageCount { label, count } => {
                    assert_eq!(
                        harness.message_count(label),
                        *count,
                        "Step '{}': Expected {} messages in {}, found {}",
                        step.name, count, label, harness.message_count(label)
                    );
                }
                Assertion::ActionRecorded { action_type, .. } => {
                    // Verify action was recorded
                    // ...
                }
            }
        }

        // Clear actions between steps
        harness.clear_actions();
    }
}
```

---

## Sample .eml Fixtures

### Simple Direct Message

```
// tests/fixtures/emails/simple/direct-message.eml

From: sender@company.com
To: me@example.com
Subject: Direct message to you
Date: Mon, 1 Jan 2024 10:00:00 +0000
Message-ID: <direct-001@company.com>
Content-Type: text/plain; charset="utf-8"

This is a direct message with no CC recipients.
```

### Email with CC

```
// tests/fixtures/emails/simple/with-cc.eml

From: sender@company.com
To: me@example.com
Cc: colleague@company.com, manager@company.com
Subject: Team update with CC
Date: Mon, 1 Jan 2024 10:00:00 +0000
Message-ID: <cc-001@company.com>
Content-Type: text/plain; charset="utf-8"

This email has CC recipients and should not match "only to me" filters.
```

### Mailing List Email

```
// tests/fixtures/emails/simple/mailing-list.eml

From: noreply@github.com
To: me@example.com
Subject: [repo/project] New issue opened
Date: Mon, 1 Jan 2024 10:00:00 +0000
Message-ID: <github-001@github.com>
List-Id: <project.lists.github.com>
List-Unsubscribe: <https://github.com/notifications/unsubscribe>
Content-Type: text/plain; charset="utf-8"

A new issue was opened in your repository.
```

### Thread Messages

```
// tests/fixtures/emails/threads/thread-01/01-initial.eml

From: alice@company.com
To: bob@company.com
Subject: Project discussion
Date: Mon, 1 Jan 2024 10:00:00 +0000
Message-ID: <thread1-msg1@company.com>
Content-Type: text/plain; charset="utf-8"

Let's discuss the project timeline.
```

```
// tests/fixtures/emails/threads/thread-01/02-reply.eml

From: bob@company.com
To: alice@company.com
Subject: Re: Project discussion
Date: Mon, 1 Jan 2024 11:00:00 +0000
Message-ID: <thread1-msg2@company.com>
In-Reply-To: <thread1-msg1@company.com>
References: <thread1-msg1@company.com>
Content-Type: text/plain; charset="utf-8"

Sure, how about next week?
```

---

## Implementation Plan

### Phase 1: Foundation (3-4 hours)

1. Create `tests/harness/` module structure
2. Implement `VirtualClock` with controllable time
3. Implement `VirtualMailbox` for in-memory state
4. Implement basic `FixtureLoader` for .eml files

### Phase 2: Mock Client (2-3 hours)

1. Implement `MockIMAPClient` with action recording
2. Create `RecordedAction` enum for all action types
3. Implement basic IMAP operations (select, search, fetch, store, move)

### Phase 3: Test Harness API (2-3 hours)

1. Create high-level `TestHarness` struct
2. Implement configuration loading for test configs
3. Add helper methods for common assertions

### Phase 4: Production Code Refactoring (3-4 hours)

1. Create `IMAPClientOps` trait for abstraction
2. Modify `StateFilter::evaluate_ttl` to accept clock parameter
3. Refactor `IMAPFilter` to use trait-based client
4. Ensure backward compatibility with existing code

### Phase 5: Test Fixtures (2 hours)

1. Create sample .eml files for various scenarios
2. Create test configuration files
3. Create scenario manifests for complex flows

### Phase 6: Integration Tests (3-4 hours)

1. Write message filter tests
2. Write state filter TTL tests
3. Write thread protection tests
4. Write full scenario tests

### Total Estimated Time: 15-20 hours

---

## Dependencies to Add

```toml
# Cargo.toml - add to [dev-dependencies]

[dev-dependencies]
tempfile = "3.20.0"  # Already present
assert_matches = "1.5"  # For pattern matching assertions
pretty_assertions = "1.4"  # For better assertion output
```

---

## Future Enhancements

1. **Snapshot Testing**: Record and replay action sequences
2. **Fuzzing**: Generate random emails to test edge cases
3. **Property-Based Testing**: Use `proptest` for invariant testing
4. **Benchmark Tests**: Performance testing with large mailboxes
5. **Coverage Reporting**: Track test coverage of filter logic
6. **GitHub Actions Integration**: Run tests in CI

---

## Conclusion

This testing harness design provides a comprehensive solution for integration testing `imap-filter-rs-v2`. Key benefits:

- **No Network Required**: All tests run against in-memory state
- **Time Control**: Virtual clock enables deterministic TTL testing
- **Full Visibility**: Action recording enables precise assertions
- **Realistic Data**: .eml fixtures enable testing with real email formats
- **Scenario Support**: Complex multi-step workflows can be tested
- **Thread Support**: Thread-aware behavior can be verified

The design maintains separation between production and test code while enabling thorough testing of the complete email processing pipeline.

