# Gmail IMAP Extensions (X-GM-*)

Gmail provides several proprietary IMAP extensions beyond RFC 3501 that enable advanced email management. These extensions are critical for thread-aware filtering.

## Overview

| Extension | Type | Description |
|-----------|------|-------------|
| `X-GM-THRID` | Attribute | Gmail Thread ID - groups messages in a conversation |
| `X-GM-MSGID` | Attribute | Gmail Message ID - unique identifier per message |
| `X-GM-LABELS` | Attribute | Gmail labels applied to the message |
| `X-GM-RAW` | Search | Raw Gmail search syntax in IMAP SEARCH |

---

## X-GM-THRID (Thread ID)

### What It Is

`X-GM-THRID` is a 64-bit unsigned integer that uniquely identifies a Gmail conversation (thread). All messages in the same conversation share the same `X-GM-THRID` value.

### Why It Matters

Gmail's threading algorithm is sophisticated - it groups messages by:
- Subject line similarity (ignoring Re:/Fwd: prefixes)
- In-Reply-To and References headers
- Participant overlap
- Timing heuristics

This is **different** from standard IMAP threading (RFC 5256) which only uses headers. Gmail's threading is more aggressive and often groups messages that header-based threading would miss.

### How to Fetch It

Include `X-GM-THRID` in your FETCH command:

```
a1 FETCH 1:* (UID X-GM-THRID RFC822.HEADER)
```

### Response Format

```
* 1 FETCH (UID 12345 X-GM-THRID 1852322999435237597 ...)
* 2 FETCH (UID 12346 X-GM-THRID 1852322999435237597 ...)  <- Same thread
* 3 FETCH (UID 12347 X-GM-THRID 1854000000000000001 ...)  <- Different thread
```

### Important: It's an Attribute, Not a Header

**Common Mistake:** Trying to extract `X-GM-THRID` from email headers.

```rust
// WRONG - X-GM-THRID is NOT in RFC822 headers
let thread_id = headers.get("X-GM-THRID");  // Always None!

// RIGHT - Parse from raw FETCH response
let raw = format!("{:?}", fetch);
let re = Regex::new(r"X-GM-THRID\s+(\d+)").unwrap();
let thread_id = re.captures(&raw).and_then(|c| c.get(1)).map(|m| m.as_str());
```

### Rust Implementation

```rust
use regex::Regex;

/// Extract Gmail Thread ID from an IMAP Fetch response.
pub fn extract_gmail_thread_id(fetch: &imap::types::Fetch) -> Option<String> {
    let raw = format!("{:?}", fetch);
    let re = Regex::new(r"X-GM-THRID\s+(\d+)").ok()?;
    re.captures(&raw)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}
```

### Thread Grouping Example

```rust
use std::collections::HashMap;

// Build a map of thread_id -> messages
let mut thread_map: HashMap<String, Vec<Message>> = HashMap::new();

for msg in messages {
    if let Some(thread_id) = &msg.thread_id {
        thread_map.entry(thread_id.clone()).or_default().push(msg);
    }
}

// Now you can process entire threads together
for (thread_id, thread_messages) in &thread_map {
    println!("Thread {} has {} messages", thread_id, thread_messages.len());
}
```

---

## X-GM-MSGID (Message ID)

### What It Is

`X-GM-MSGID` is a 64-bit unsigned integer that uniquely identifies a single Gmail message. Unlike the RFC 5322 `Message-ID` header, this is Gmail's internal identifier.

### When to Use It

- Deduplication across folders/labels
- Precise message referencing
- API correlation (matches Gmail API message IDs)

### How to Fetch It

```
a1 FETCH 1:* (UID X-GM-MSGID)
```

### Response Format

```
* 1 FETCH (UID 12345 X-GM-MSGID 1852322999435237598)
```

### Relationship to X-GM-THRID

- One `X-GM-THRID` can have many `X-GM-MSGID` values
- Each message has exactly one of each
- `X-GM-MSGID` is always unique; `X-GM-THRID` is shared within a thread

---

## X-GM-LABELS

### What It Is

A list of Gmail labels applied to a message. Includes both system labels and user-created labels.

### How to Fetch It

```
a1 FETCH 1:* (UID X-GM-LABELS)
```

### Response Format

```
* 1 FETCH (UID 12345 X-GM-LABELS ("INBOX" "Important" "\\Starred" "Projects/Work"))
```

### Label Types

| Label | Type | Notes |
|-------|------|-------|
| `INBOX` | System | Message is in inbox |
| `\\Starred` | System | Starred (note backslash) |
| `\\Important` | System | Marked important |
| `\\Sent` | System | In sent mail |
| `\\Draft` | System | Draft message |
| `\\Spam` | System | In spam folder |
| `\\Trash` | System | In trash |
| `Projects/Work` | User | Custom label (can have `/` hierarchy) |

### Modifying Labels

Use `STORE` with `+X-GM-LABELS` or `-X-GM-LABELS`:

```
a1 UID STORE 12345 +X-GM-LABELS ("MyLabel")
a1 UID STORE 12345 -X-GM-LABELS ("\\Inbox")
```

### Rust Implementation

```rust
pub fn get_labels(session: &mut Session<T>, uid: u32) -> Result<HashSet<String>> {
    let fetches = session.fetch(uid.to_string(), "X-GM-LABELS")?;
    let mut labels = HashSet::new();

    for f in &fetches {
        let raw = format!("{:?}", f);
        if let Some(start) = raw.find("X-GM-LABELS (") {
            let rest = &raw[start + 13..];
            if let Some(end) = rest.find(')') {
                for lbl in rest[..end].split_whitespace() {
                    labels.insert(lbl.trim_matches('"').to_string());
                }
            }
        }
    }
    Ok(labels)
}

pub fn add_label(session: &mut Session<T>, uid: u32, label: &str) -> Result<()> {
    let cmd = format!("+X-GM-LABELS.SILENT (\"{}\")", label);
    session.uid_store(uid.to_string(), cmd)?;
    Ok(())
}
```

---

## X-GM-RAW (Raw Search)

### What It Is

Allows using Gmail's native search syntax within IMAP SEARCH commands.

### How to Use It

```
a1 SEARCH X-GM-RAW "from:boss@company.com has:attachment"
a1 SEARCH X-GM-RAW "in:inbox is:unread newer_than:7d"
```

### Supported Operators

All Gmail search operators work:
- `from:`, `to:`, `cc:`, `bcc:`
- `subject:`, `has:attachment`, `filename:`
- `is:starred`, `is:unread`, `is:important`
- `in:inbox`, `in:trash`, `label:MyLabel`
- `before:`, `after:`, `older_than:`, `newer_than:`
- `larger:`, `smaller:`
- Boolean: `OR`, `AND`, `-` (NOT), `()` grouping

### Example: Find Unread Threads from Last Week

```
a1 SEARCH X-GM-RAW "is:unread newer_than:7d"
```

---

## Thread-Aware Filtering Strategy

### Building the Thread Map

```rust
pub fn build_thread_map(messages: &[Message]) -> HashMap<String, Vec<Message>> {
    let mut thread_map = HashMap::new();

    for msg in messages {
        if let Some(thread_id) = &msg.thread_id {
            thread_map.entry(thread_id.clone()).or_default().push(msg.clone());
        }
    }

    thread_map
}
```

### Thread-Aware TTL Evaluation

When evaluating TTL for thread-aware expiry:

```rust
// A thread expires when its NEWEST message has exceeded TTL
fn thread_expired(thread_msgs: &[Message], ttl: Duration) -> bool {
    let newest = thread_msgs.iter().max_by_key(|m| &m.date);

    if let Some(msg) = newest {
        let msg_age = Utc::now() - parse_date(&msg.date);
        return msg_age > ttl;
    }

    false
}
```

### Thread Protection

If ANY message in a thread is protected (starred/important), protect the entire thread:

```rust
fn thread_protected(thread_msgs: &[Message]) -> bool {
    thread_msgs.iter().any(|m| {
        m.labels.contains(&Label::Starred) || m.labels.contains(&Label::Important)
    })
}
```

---

## Fallback: Standard IMAP Threading

For non-Gmail servers, use standard RFC headers:

| Header | Purpose |
|--------|---------|
| `Message-ID` | Unique message identifier |
| `In-Reply-To` | Parent message ID |
| `References` | Chain of ancestor message IDs |

Build thread groups by finding connected components in the reference graph.

```rust
// If no X-GM-THRID, fall back to header-based threading
if msg.thread_id.is_none() {
    // Use Message-ID, In-Reply-To, References to build thread graph
    let thread_id = compute_thread_from_headers(
        &msg.message_id,
        &msg.in_reply_to,
        &msg.references
    );
}
```

---

## Common Pitfalls

### 1. Treating X-GM-THRID as a Header

❌ Wrong:
```rust
headers.get("X-GM-THRID")  // Always returns None
```

✅ Right:
```rust
extract_gmail_thread_id(&fetch)  // Parse from FETCH response
```

### 2. Not Requesting X-GM-THRID in FETCH

❌ Wrong:
```
FETCH 1:* (UID RFC822.HEADER)
```

✅ Right:
```
FETCH 1:* (UID X-GM-THRID RFC822.HEADER)
```

### 3. Assuming Thread ID is in Message Headers

The `X-GM-THRID` is an IMAP **attribute** returned by the server, not embedded in the message itself. It exists only in Gmail's IMAP protocol response.

### 4. Using Standard Threading with Gmail

Gmail's threading is more sophisticated than RFC 5256. Use `X-GM-THRID` for accurate thread grouping with Gmail.

---

## References

- [Gmail IMAP Extensions](https://developers.google.com/gmail/imap/imap-extensions)
- [RFC 3501 - IMAP4rev1](https://tools.ietf.org/html/rfc3501)
- [RFC 5256 - IMAP SORT and THREAD](https://tools.ietf.org/html/rfc5256)

