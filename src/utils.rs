// src/utils.rs

use eyre::{Result, eyre};
use imap::Session;
use native_tls::TlsStream;
use std::net::TcpStream;
use log::{info, debug};
use std::collections::HashSet;
use chrono::{DateTime, Duration, Utc, FixedOffset};
use std::io::{Read, Write};
use regex::Regex;

/// Parse a string like "7d" into a chrono::Duration of days.
/// Returns an error if the format is unsupported.
pub fn parse_days(s: &str) -> Result<Duration> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix('d') {
        let days: i64 = num.parse()
            .map_err(|e| eyre!("Invalid TTL duration '{}': {}", s, e))?;
        Ok(Duration::days(days))
    } else {
        Err(eyre!(
            "Unsupported TTL format '{}'; expected '<n>d'",
            s
        ))
    }
}

/// Validates that an IMAP search query uses supported flags and syntax.
pub fn validate_imap_query(query: &str) -> Result<()> {
    let valid_tokens = [
        "ALL", "ANSWERED", "DELETED", "DRAFT", "FLAGGED", "NEW", "OLD",
        "RECENT", "SEEN", "UNANSWERED", "UNDELETED", "UNDRAFT", "UNFLAGGED", "UNSEEN",
        "X-GM-LABELS", "X-GM-RAW", "X-GM-THRID", "X-GM-MSGID",
        "INBOX", // treated specially by some servers
        "NOT", "OR", "AND"
    ];

    if query.trim().is_empty() {
        return Err(eyre!("IMAP query must not be empty"));
    }

    if query.contains('\\') {
        // allow known escaped flags
        let known = ["\\Seen","\\Deleted","\\Flagged","\\Draft","\\Answered"];
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
pub fn ensure_label_exists<T>(
    client: &mut Session<T>,
    label: &str,
) -> Result<()>
where
    T: Read + Write,
{
    let list = client.list(None, Some("*"))?;
    let exists = list.iter().any(|mb| mb.name() == label);
    if !exists {
        info!("Creating missing label '{}'", label);
        client.create(label)
            .map_err(|e| eyre!("Failed to create label '{}': {:?}", label, e))?;
        info!("Label '{}' created", label);
    }
    Ok(())
}

/// Returns the set of Gmail labels on this message (by UID).
pub fn get_labels<T>(
    session: &mut Session<T>,
    uid: u32,
) -> Result<HashSet<String>>
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

/// Add a label to the message, creating the label if needed.
pub fn set_label<T>(
    client: &mut Session<T>,
    uid: u32,
    label: &str,
    subject: &str,
) -> Result<()>
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

/// Remove a label from the message.
pub fn del_label<T>(
    client: &mut Session<T>,
    uid: u32,
    label: &str,
    subject: &str,
) -> Result<()>
where
    T: Read + Write,
{
    // SILENT to suppress the untagged FETCH
    let cmd = format!(
        "-X-GM-LABELS.SILENT (\"{}\")",
        label.replace('\\', "\\\\").replace('"', "\\\"")
    );
    client
        .uid_store(uid.to_string(), cmd)
        .map(|_| ())
        .map_err(|e| eyre!("Failed to remove label '{}' from UID {}: {:?} | {}", label, uid, e, subject))
}

/// “Move” a message by moving it server-side from INBOX → `label`.
/// Uses the UID MOVE extension (Gmail supports it), so you never have
/// to manually remove “INBOX” yourself.
pub fn uid_move_gmail<T>(
    client: &mut Session<T>,
    uid: u32,
    label: &str,
    subject: &str,
) -> Result<()>
where
    T: Read + Write,
{
    // make sure the destination mailbox/label exists
    ensure_label_exists(client, label)?;

    // this sends: `a1 UID MOVE 12345 "Purgatory"`
    client
        .uid_mv(uid.to_string(), label)
        .map(|_| ())
        .map_err(|e| eyre!(
            "Failed to MOVE UID {} → `{}`: {:?} | {}",
            uid, label, e, subject
        ))
}
