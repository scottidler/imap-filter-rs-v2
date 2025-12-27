// src/client_ops.rs
//
// Clock trait abstraction for time-based operations.
// Allows production code to use real time or virtual time for testing.

use chrono::{DateTime, Utc};

/// Trait for time providers.
/// Allows production code to use real time or virtual time for testing.
/// Used by StateFilter for TTL evaluation and by ThreadProcessor for age calculations.
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
