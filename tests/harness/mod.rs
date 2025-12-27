// tests/harness/mod.rs
//
// Integration testing harness for imap-filter-rs-v2.
// Provides in-memory IMAP simulation and time control for testing.

pub mod fixtures;
pub mod mock_client;
pub mod test_harness;
pub mod virtual_clock;
pub mod virtual_mailbox;

pub use fixtures::{EmailFixture, FixtureError, FixtureLoader};
pub use mock_client::{MockIMAPClient, RecordedAction};
pub use test_harness::TestHarness;
pub use virtual_clock::{Clock, RealClock, VirtualClock};
pub use virtual_mailbox::{MailboxMessage, MoveRecord, VirtualMailbox};
