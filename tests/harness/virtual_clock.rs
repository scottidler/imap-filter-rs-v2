// tests/harness/virtual_clock.rs
//
// Virtual clock for testing TTL-based state transitions.
// Allows tests to control time without waiting for real time to pass.

use chrono::{DateTime, Duration, Utc};
use std::sync::{Arc, RwLock};

/// A clock that can be controlled for testing.
/// Thread-safe via Arc<RwLock<...>>.
#[derive(Clone)]
pub struct VirtualClock {
    inner: Arc<RwLock<DateTime<Utc>>>,
}

impl VirtualClock {
    /// Create a new virtual clock set to the current real time.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Utc::now())),
        }
    }

    /// Create a virtual clock set to a specific time.
    pub fn at(time: DateTime<Utc>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(time)),
        }
    }

    /// Get the current virtual time.
    pub fn now(&self) -> DateTime<Utc> {
        *self.inner.read().unwrap()
    }

    /// Advance time by the given duration.
    pub fn advance(&self, duration: Duration) {
        let mut guard = self.inner.write().unwrap();
        *guard += duration;
    }

    /// Advance time by the given number of days.
    pub fn advance_days(&self, days: i64) {
        self.advance(Duration::days(days));
    }

    /// Set the clock to a specific time.
    pub fn set(&self, time: DateTime<Utc>) {
        *self.inner.write().unwrap() = time;
    }

    /// Rewind time by the given duration.
    pub fn rewind(&self, duration: Duration) {
        let mut guard = self.inner.write().unwrap();
        *guard -= duration;
    }
}

impl Default for VirtualClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for components that need to access the current time.
/// This allows production code to use real time while tests use virtual time.
pub trait Clock: Clone + Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

impl Clock for VirtualClock {
    fn now(&self) -> DateTime<Utc> {
        self.now()
    }
}

/// Real clock for production use - simply returns Utc::now().
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
    fn test_virtual_clock_starts_at_now() {
        let before = Utc::now();
        let clock = VirtualClock::new();
        let after = Utc::now();

        let clock_time = clock.now();
        assert!(clock_time >= before);
        assert!(clock_time <= after);
    }

    #[test]
    fn test_virtual_clock_at_specific_time() {
        let specific_time = DateTime::parse_from_rfc3339("2024-01-15T10:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let clock = VirtualClock::at(specific_time);
        assert_eq!(clock.now(), specific_time);
    }

    #[test]
    fn test_virtual_clock_advance_days() {
        let start_time = DateTime::parse_from_rfc3339("2024-01-15T10:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let clock = VirtualClock::at(start_time);
        clock.advance_days(7);

        let expected = DateTime::parse_from_rfc3339("2024-01-22T10:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        assert_eq!(clock.now(), expected);
    }

    #[test]
    fn test_virtual_clock_advance_duration() {
        let start_time = DateTime::parse_from_rfc3339("2024-01-15T10:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let clock = VirtualClock::at(start_time);
        clock.advance(Duration::hours(12));

        let expected = DateTime::parse_from_rfc3339("2024-01-15T22:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        assert_eq!(clock.now(), expected);
    }

    #[test]
    fn test_virtual_clock_rewind() {
        let start_time = DateTime::parse_from_rfc3339("2024-01-15T10:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let clock = VirtualClock::at(start_time);
        clock.rewind(Duration::days(3));

        let expected = DateTime::parse_from_rfc3339("2024-01-12T10:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        assert_eq!(clock.now(), expected);
    }

    #[test]
    fn test_virtual_clock_set() {
        let clock = VirtualClock::new();
        let new_time = DateTime::parse_from_rfc3339("2025-06-01T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        clock.set(new_time);
        assert_eq!(clock.now(), new_time);
    }

    #[test]
    fn test_virtual_clock_is_clone() {
        let clock1 = VirtualClock::new();
        let clock2 = clock1.clone();

        // Both clones share the same internal state
        clock1.advance_days(5);
        assert_eq!(clock1.now(), clock2.now());
    }

    #[test]
    fn test_real_clock_returns_current_time() {
        let before = Utc::now();
        let clock = RealClock;
        let clock_time = clock.now();
        let after = Utc::now();

        assert!(clock_time >= before);
        assert!(clock_time <= after);
    }

    #[test]
    fn test_clock_trait_works_with_virtual_clock() {
        fn use_clock<C: Clock>(clock: &C) -> DateTime<Utc> {
            clock.now()
        }

        let specific_time = DateTime::parse_from_rfc3339("2024-01-15T10:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let clock = VirtualClock::at(specific_time);
        assert_eq!(use_clock(&clock), specific_time);
    }
}

