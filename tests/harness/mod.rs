// tests/harness/mod.rs
//
// Integration testing harness for imap-filter-rs-v2.
// Provides in-memory IMAP simulation and time control for testing.

pub mod fixtures;
pub mod virtual_clock;
pub mod virtual_mailbox;

pub use fixtures::{EmailFixture, FixtureLoader};

// TEMPORARY: FixtureError will be used in Phase 2+ for error handling in integration tests
#[allow(unused_imports)]
pub use fixtures::FixtureError;
pub use virtual_clock::{Clock, RealClock, VirtualClock};
pub use virtual_mailbox::{MailboxMessage, MoveRecord, VirtualMailbox};

