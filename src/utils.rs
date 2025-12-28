// src/utils.rs

use chrono::Duration;
use eyre::{eyre, Result};
use imap::Session;
use log::{debug, info, warn};
use regex::Regex;
use std::collections::HashSet;
use std::io::{Read, Write};
use std::thread;
use std::time::Duration as StdDuration;

/// Gmail/IMAP error classification
#[derive(Debug, Clone, PartialEq)]
pub enum ImapErrorKind {
    /// Rate limited by server - should back off and retry
    RateLimit,
    /// Temporary server error - may succeed on retry
    TransientError,
    /// Connection lost - need to reconnect
    ConnectionLost,
    /// Message not found or already moved
    MessageNotFound,
    /// Permanent error - don't retry
    PermanentError,
    /// Unknown error
    Unknown,
}

impl std::fmt::Display for ImapErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImapErrorKind::RateLimit => write!(f, "RATE_LIMITED"),
            ImapErrorKind::TransientError => write!(f, "TRANSIENT_ERROR"),
            ImapErrorKind::ConnectionLost => write!(f, "CONNECTION_LOST"),
            ImapErrorKind::MessageNotFound => write!(f, "MESSAGE_NOT_FOUND"),
            ImapErrorKind::PermanentError => write!(f, "PERMANENT_ERROR"),
            ImapErrorKind::Unknown => write!(f, "UNKNOWN"),
        }
    }
}

/// Classify an IMAP error based on its message/type
pub fn classify_imap_error(error: &imap::Error) -> ImapErrorKind {
    let error_str = format!("{:?}", error);
    let error_lower = error_str.to_lowercase();

    // Check for rate limiting indicators
    if error_lower.contains("too many")
        || error_lower.contains("rate")
        || error_lower.contains("throttl")
        || error_lower.contains("quota")
        || error_lower.contains("try again later")
    {
        return ImapErrorKind::RateLimit;
    }

    // Check for transient/system errors
    if error_lower.contains("system error")
        || error_lower.contains("temporary")
        || error_lower.contains("try again")
        || error_lower.contains("service unavailable")
        || error_lower.contains("internal error")
    {
        return ImapErrorKind::TransientError;
    }

    // Check for connection issues
    if error_lower.contains("connection")
        || error_lower.contains("disconnected")
        || error_lower.contains("broken pipe")
        || error_lower.contains("reset by peer")
        || error_lower.contains("timed out")
        || error_lower.contains("eof")
    {
        return ImapErrorKind::ConnectionLost;
    }

    // Check for message not found
    if error_lower.contains("no such message")
        || error_lower.contains("not found")
        || error_lower.contains("nonexistent")
        || error_lower.contains("expunged")
    {
        return ImapErrorKind::MessageNotFound;
    }

    // Check for permanent errors
    if error_lower.contains("permission denied")
        || error_lower.contains("invalid")
        || error_lower.contains("not supported")
    {
        return ImapErrorKind::PermanentError;
    }

    ImapErrorKind::Unknown
}

/// Retry configuration
const MAX_RETRIES: u32 = 3;
const INITIAL_BACKOFF_MS: u64 = 1000;
const BACKOFF_MULTIPLIER: u64 = 2;

/// Execute an IMAP operation with retry logic
fn with_retry<F, T>(operation_name: &str, uid: u32, mut operation: F) -> Result<T>
where
    F: FnMut() -> std::result::Result<T, imap::Error>,
{
    let mut attempt = 0;
    let mut backoff_ms = INITIAL_BACKOFF_MS;

    loop {
        attempt += 1;
        match operation() {
            Ok(result) => return Ok(result),
            Err(e) => {
                let error_kind = classify_imap_error(&e);
                let should_retry = matches!(error_kind, ImapErrorKind::RateLimit | ImapErrorKind::TransientError);

                warn!(
                    "⚠️  IMAP Error during {} for UID {}: [{:?}] {:?} (attempt {}/{})",
                    operation_name, uid, error_kind, e, attempt, MAX_RETRIES
                );

                if !should_retry || attempt >= MAX_RETRIES {
                    return Err(eyre!(
                        "{} failed for UID {} after {} attempts: [{}] {:?}",
                        operation_name,
                        uid,
                        attempt,
                        error_kind,
                        e
                    ));
                }

                // Log specific guidance for rate limits
                if error_kind == ImapErrorKind::RateLimit {
                    warn!(
                        "🚦 Rate limited by Gmail. Backing off for {}ms before retry...",
                        backoff_ms
                    );
                }

                thread::sleep(StdDuration::from_millis(backoff_ms));
                backoff_ms *= BACKOFF_MULTIPLIER;
            }
        }
    }
}

/// Parse a string like "7d" into a chrono::Duration of days.
/// Returns an error if the format is unsupported.
pub fn parse_days(s: &str) -> Result<Duration> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix('d') {
        let days: i64 = num.parse().map_err(|e| eyre!("Invalid TTL duration '{}': {}", s, e))?;
        Ok(Duration::days(days))
    } else {
        Err(eyre!("Unsupported TTL format '{}'; expected '<n>d'", s))
    }
}

/// Ensures the given label exists on the server, creating it if necessary.
pub fn ensure_label_exists<T>(client: &mut Session<T>, label: &str) -> Result<()>
where
    T: Read + Write,
{
    let list = client.list(None, Some("*"))?;
    let exists = list.iter().any(|mb| mb.name() == label);
    if !exists {
        info!("Creating missing label '{}'", label);
        client
            .create(label)
            .map_err(|e| eyre!("Failed to create label '{}': {:?}", label, e))?;
        info!("Label '{}' created", label);
    }
    Ok(())
}

/// Returns the set of Gmail labels on this message (by UID).
pub fn get_labels<T>(session: &mut Session<T>, uid: u32) -> Result<HashSet<String>>
where
    T: Read + Write,
{
    let fetches = session.fetch(uid.to_string(), "X-GM-LABELS")?;
    let mut labels = HashSet::new();
    for f in fetches.iter() {
        let raw = format!("{:?}", f);
        debug!("raw FETCH: {}", raw);
        if let Some(start) = raw.find("X-GM-LABELS (") {
            let rest = &raw[start + 13..];
            if let Some(end) = rest.find(')') {
                for lbl in rest[..end].split_whitespace() {
                    let lbl = lbl.trim_matches('"');
                    if !lbl.is_empty() {
                        labels.insert(lbl.to_string());
                    }
                }
            }
        }
    }
    Ok(labels)
}

/// Helper to extract a Gmail extension field value from raw FETCH output.
/// The value is expected to be a numeric ID following the field name.
#[allow(dead_code)] // Used in tests and may be useful for future Gmail-specific features
fn extract_gmail_extension(raw: &str, field: &str) -> Option<String> {
    let pattern = format!(r"{}\s+(\d+)", field);
    let re = Regex::new(&pattern).ok()?;
    re.captures(raw)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}

/// Add a label to the message, creating the label if needed.
/// Includes retry logic for transient errors and rate limiting.
pub fn set_label<T>(client: &mut Session<T>, uid: u32, label: &str, subject: &str) -> Result<()>
where
    T: Read + Write,
{
    let current = get_labels(client, uid)?;
    if current.contains(label) {
        debug!("UID {} already has label '{}' (subject={})", uid, label, subject);
        return Ok(());
    }
    ensure_label_exists(client, label)?;
    // SILENT to suppress the untagged FETCH
    let cmd = format!(
        "+X-GM-LABELS.SILENT (\"{}\")",
        label.replace('\\', "\\\\").replace('"', "\\\"")
    );
    debug!("before client.uid_store: cmd={}", cmd);

    let cmd_owned = cmd.clone();
    with_retry(&format!("SET_LABEL {}", label), uid, || {
        client.uid_store(uid.to_string(), &cmd_owned)
    })
    .map(|_| ())
    .map_err(|e| eyre!("{} | subject: {}", e, subject))
}

/// "Move" a message by moving it server-side from INBOX → `label`.
/// Uses the UID MOVE extension (Gmail supports it), so you never have
/// to manually remove "INBOX" yourself.
/// Includes retry logic for transient errors and rate limiting.
pub fn uid_move_gmail<T>(client: &mut Session<T>, uid: u32, label: &str, subject: &str) -> Result<()>
where
    T: Read + Write,
{
    // make sure the destination mailbox/label exists
    ensure_label_exists(client, label)?;

    // this sends: `a1 UID MOVE 12345 "Purgatory"` with retry logic
    let label_owned = label.to_string();
    with_retry(&format!("MOVE → {}", label), uid, || {
        client.uid_mv(uid.to_string(), &label_owned)
    })
    .map(|_| ())
    .map_err(|e| eyre!("{} | subject: {}", e, subject))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_days_valid() {
        assert_eq!(parse_days("7d").unwrap(), Duration::days(7));
        assert_eq!(parse_days("1d").unwrap(), Duration::days(1));
        assert_eq!(parse_days("30d").unwrap(), Duration::days(30));
        assert_eq!(parse_days("365d").unwrap(), Duration::days(365));
        assert_eq!(parse_days("  7d  ").unwrap(), Duration::days(7)); // with whitespace
    }

    #[test]
    fn test_parse_days_invalid() {
        assert!(parse_days("7").is_err()); // missing 'd'
        assert!(parse_days("d").is_err()); // missing number
        assert!(parse_days("7h").is_err()); // wrong suffix
        assert!(parse_days("").is_err()); // empty
        assert!(parse_days("abc").is_err()); // not a number
    }

    #[test]
    fn test_extract_gmail_extension() {
        let raw = "Fetch { uid: Some(12345), X-GM-THRID 1852322999435237597, X-GM-MSGID 1852322999435237598 }";
        assert_eq!(
            extract_gmail_extension(raw, "X-GM-THRID"),
            Some("1852322999435237597".to_string())
        );
        assert_eq!(
            extract_gmail_extension(raw, "X-GM-MSGID"),
            Some("1852322999435237598".to_string())
        );
        assert_eq!(extract_gmail_extension(raw, "X-GM-UNKNOWN"), None);
    }

    #[test]
    fn test_extract_gmail_extension_no_match() {
        let raw = "Fetch { uid: Some(12345) }";
        assert_eq!(extract_gmail_extension(raw, "X-GM-THRID"), None);
    }

    #[test]
    fn test_extract_gmail_extension_whitespace_variations() {
        // Multiple spaces
        let raw1 = "X-GM-THRID  1234567890";
        assert_eq!(
            extract_gmail_extension(raw1, "X-GM-THRID"),
            Some("1234567890".to_string())
        );

        // Tab
        let raw2 = "X-GM-THRID\t9876543210";
        assert_eq!(
            extract_gmail_extension(raw2, "X-GM-THRID"),
            Some("9876543210".to_string())
        );
    }
}
