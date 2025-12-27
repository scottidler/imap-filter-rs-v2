// src/client_ops.rs
//
// Trait abstraction for IMAP client operations.
// Allows production code to work with real IMAP clients or test mocks.

use chrono::{DateTime, Utc};
use eyre::Result;

use crate::message::Message;

/// Trait for IMAP client operations.
/// This abstraction allows the same filter logic to work with:
/// - Real IMAP sessions (production)
/// - Mock clients (testing)
// TEMPORARY: Will be used in Phase 5+ for full production code integration
#[allow(dead_code)]
pub trait IMAPClientOps {
    /// Select a mailbox/folder
    fn select(&mut self, mailbox: &str) -> Result<()>;

    /// Search for messages matching a query
    fn search(&mut self, query: &str) -> Result<Vec<u32>>;

    /// Fetch messages by sequence set
    fn fetch_messages(&mut self, seq_set: &str) -> Result<Vec<Message>>;

    /// Add flags to a message
    fn uid_store_add_flags(&mut self, uid: u32, flags: &str) -> Result<()>;

    /// Remove flags from a message
    fn uid_store_remove_flags(&mut self, uid: u32, flags: &str) -> Result<()>;

    /// Move a message to another folder
    fn uid_move(&mut self, uid: u32, destination: &str) -> Result<()>;

    /// Ensure a label/folder exists, creating if necessary
    fn ensure_label_exists(&mut self, label: &str) -> Result<()>;

    /// Get current labels on a message
    fn get_labels(&mut self, uid: u32) -> Result<std::collections::HashSet<String>>;

    /// Set a label on a message (Gmail-specific)
    fn set_label(&mut self, uid: u32, label: &str, subject: &str) -> Result<()>;

    /// Logout from the session
    fn logout(&mut self) -> Result<()>;

    /// Expunge deleted messages
    fn expunge(&mut self) -> Result<()>;
}

/// Trait for time providers.
/// Allows production code to use real time or virtual time for testing.
pub trait Clock: Clone + Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

/// Real clock implementation using system time.
#[derive(Clone, Default)]
pub struct RealClock;

impl Clock for RealClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_real_clock_returns_current_time() {
        let clock = RealClock;
        let before = Utc::now();
        let clock_time = clock.now();
        let after = Utc::now();

        assert!(clock_time >= before);
        assert!(clock_time <= after);
    }
}
