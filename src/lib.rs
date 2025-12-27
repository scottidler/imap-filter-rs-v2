// src/lib.rs
//
// Library entry point for imap-filter.
// Re-exports modules needed by integration tests.

pub mod cfg;
pub mod client_ops;
pub mod message;
pub mod utils;

// Re-export Clock trait for easy access
pub use client_ops::{Clock, RealClock};
