// src/utils.rs

use chrono::Duration;
use eyre::{eyre, Result};
use imap::Session;
use log::{debug, info};
use regex::Regex;
use std::collections::HashSet;
use std::io::{Read, Write};

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

/// Validates that an IMAP search query uses supported flags and syntax.
#[allow(dead_code)] // Will be used when query validation is added to config loading
pub fn validate_imap_query(query: &str) -> Result<()> {
    let valid_tokens = [
        "ALL",
        "ANSWERED",
        "DELETED",
        "DRAFT",
        "FLAGGED",
        "NEW",
        "OLD",
        "RECENT",
        "SEEN",
        "UNANSWERED",
        "UNDELETED",
        "UNDRAFT",
        "UNFLAGGED",
        "UNSEEN",
        "X-GM-LABELS",
        "X-GM-RAW",
        "X-GM-THRID",
        "X-GM-MSGID",
        "INBOX", // treated specially by some servers
        "NOT",
        "OR",
        "AND",
    ];

    if query.trim().is_empty() {
        return Err(eyre!("IMAP query must not be empty"));
    }

    if query.contains('\\') {
        // allow known escaped flags
        let known = ["\\Seen", "\\Deleted", "\\Flagged", "\\Draft", "\\Answered"];
        if !known.iter().any(|&f| query.contains(f)) {
            return Err(eyre!("Unknown or improperly escaped IMAP flag in query: {}", query));
        }
    }

    for token in query.split_whitespace() {
        let t = token.trim_matches(|c| c == '(' || c == ')' || c == '"');
        if t.starts_with("X-GM-LABELS")
            || valid_tokens.iter().any(|&v| v.eq_ignore_ascii_case(t))
            || t.starts_with('\\')
            || t.chars().all(char::is_alphanumeric)
        {
            continue;
        } else {
            return Err(eyre!("Unsupported or malformed token in IMAP query: '{}'", token));
        }
    }

    Ok(())
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
    for f in &fetches {
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

/// Extract Gmail Thread ID (X-GM-THRID) from an IMAP Fetch response.
///
/// Gmail provides X-GM-THRID as an IMAP extension attribute (not a header).
/// It must be parsed from the raw FETCH response debug output.
///
/// Example response: "... X-GM-THRID 1852322999435237597 ..."
pub fn extract_gmail_thread_id(fetch: &imap::types::Fetch) -> Option<String> {
    let raw = format!("{:?}", fetch);
    extract_gmail_extension(&raw, "X-GM-THRID")
}

/// Helper to extract a Gmail extension field value from raw FETCH output.
/// The value is expected to be a numeric ID following the field name.
fn extract_gmail_extension(raw: &str, field: &str) -> Option<String> {
    let pattern = format!(r"{}\s+(\d+)", field);
    let re = Regex::new(&pattern).ok()?;
    re.captures(raw)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}

/// Add a label to the message, creating the label if needed.
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
    client
        .uid_store(uid.to_string(), cmd)
        .map(|_| ())
        .map_err(|e| eyre!("Failed to add label '{}' to UID {}: {:?} | {}", label, uid, e, subject))
}

/// "Move" a message by moving it server-side from INBOX → `label`.
/// Uses the UID MOVE extension (Gmail supports it), so you never have
/// to manually remove "INBOX" yourself.
pub fn uid_move_gmail<T>(client: &mut Session<T>, uid: u32, label: &str, subject: &str) -> Result<()>
where
    T: Read + Write,
{
    // make sure the destination mailbox/label exists
    ensure_label_exists(client, label)?;

    // this sends: `a1 UID MOVE 12345 "Purgatory"`
    client
        .uid_mv(uid.to_string(), label)
        .map(|_| ())
        .map_err(|e| eyre!("Failed to MOVE UID {} → `{}`: {:?} | {}", uid, label, e, subject))
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
    fn test_validate_imap_query_valid() {
        assert!(validate_imap_query("ALL").is_ok());
        assert!(validate_imap_query("SEEN").is_ok());
        assert!(validate_imap_query("NOT SEEN").is_ok());
        assert!(validate_imap_query("\\Seen").is_ok());
        assert!(validate_imap_query("\\Flagged").is_ok());
        assert!(validate_imap_query("X-GM-LABELS test").is_ok());
    }

    #[test]
    fn test_validate_imap_query_invalid() {
        assert!(validate_imap_query("").is_err()); // empty
        assert!(validate_imap_query("   ").is_err()); // whitespace only
        assert!(validate_imap_query("\\Unknown").is_err()); // unknown escaped flag
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
