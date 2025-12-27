# imap-filter-rs-v2: Design & Architecture

## Overview

`imap-filter-rs` is an IMAP email filtering tool designed to provide filtering capabilities that Gmail's native filters cannot offer. It connects to Gmail via IMAP, fetches messages, applies custom filter rules, and performs actions like starring, flagging, moving, or deleting messages.

### Key Differentiators from Gmail Native Filters

| Capability | Gmail Native | imap-filter-rs |
|------------|--------------|----------------|
| "Only to me" (sole recipient, no CC) | ❌ | ✅ |
| TTL-based message expiry | ❌ | ✅ |
| Thread-aware state transitions | ❌ | ✅ |
| Glob pattern matching on addresses | ❌ | ✅ |
| Multi-stage purgatory workflow | ❌ | ✅ |

---

## Architecture

### High-Level Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│                         imap-filter-rs                               │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌──────────┐    ┌──────────────┐    ┌───────────────────────────┐  │
│  │  CLI     │───▶│ Config       │───▶│ IMAPFilter                │  │
│  │  Parser  │    │ Loader       │    │                           │  │
│  └──────────┘    └──────────────┘    │  ┌─────────────────────┐  │  │
│                                       │  │ Phase 1:            │  │  │
│                                       │  │ MessageFilters      │  │  │
│  ┌──────────────────────────────┐    │  │ (Star/Flag/Move)    │  │  │
│  │ Gmail IMAP Server            │◀──▶│  └─────────────────────┘  │  │
│  │ - X-GM-THRID (thread ID)     │    │  ┌─────────────────────┐  │  │
│  │ - X-GM-LABELS               │    │  │ Phase 2:            │  │  │
│  │ - X-GM-MSGID                │    │  │ StateFilters        │  │  │
│  └──────────────────────────────┘    │  │ (TTL/Expire/Move)   │  │  │
│                                       │  └─────────────────────┘  │  │
│                                       └───────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

### Module Structure

```
src/
├── main.rs              # Entry point, logging setup, IMAP connection
├── cli.rs               # Command-line argument parsing (clap)
├── imap_filter.rs       # Core filter execution engine
├── message.rs           # Message struct and header parsing
├── thread.rs            # Thread grouping and thread-aware processing
├── utils.rs             # IMAP utilities (labels, moves, Gmail extensions)
└── cfg/
    ├── mod.rs           # Module exports
    ├── config.rs        # YAML config loading and deserialization
    ├── label.rs         # Gmail label enum (Inbox, Starred, Custom, etc.)
    ├── message_filter.rs # MessageFilter struct and matching logic
    ├── state_filter.rs  # StateFilter struct with TTL evaluation
    └── secure.rs        # SecureString deserialization for passwords
```

---

## Filter Types

### 1. MessageFilters (Phase 1)

MessageFilters match incoming messages based on headers and apply immediate actions.

**Matching Criteria:**
- `to`: Glob patterns for To recipients
- `cc`: Glob patterns for CC recipients (empty = require no CC)
- `from`: Glob patterns for sender
- `subject`: Glob patterns for subject line
- `labels`: Required/excluded labels

**Actions:**
- `Star`: Add `\Starred` flag (appears in Gmail Starred)
- `Flag`: Add `\Important` flag (appears in Gmail Important)
- `Move`: Move message to a different label/folder

**Example: "Only to me" filter**
```yaml
message-filters:
  - only-me-star:
      to: ['scott.idler@tatari.tv']
      to_exact: true      # Must be sole recipient (proposed feature)
      cc: []              # No CC recipients
      from: '*@tatari.tv'
      label: INBOX
      action: Star
```

### 2. StateFilters (Phase 2)

StateFilters evaluate message state (labels, age) and apply time-based transitions.

**Matching Criteria:**
- `labels`: Messages must have any of these labels

**TTL Types:**
- `Keep`: Never expire (protect from all expiry)
- `<n>d`: Expire after n days
- `{ read: <n>d, unread: <m>d }`: Different TTL for read vs unread

**Actions:**
- `Move`: Move to destination label
- `Delete`: Mark as deleted

**Example: Multi-stage expiry workflow**
```yaml
state-filters:
  # Protected messages
  - Starred:
      labels: [Important, Starred]
      ttl: Keep

  # Age out read messages faster than unread
  - Cull:
      ttl:
        read: 7d
        unread: 21d
      action: Purgatory

  # Final deletion after grace period
  - Purge:
      label: Purgatory
      ttl: 3d
      action:
        Move: Oblivion
```

---

## Thread Support

### Current State: BROKEN ❌

The current implementation attempts to extract Gmail's thread ID (`X-GM-THRID`) from email headers:

```rust
// src/message.rs - CURRENT (BROKEN)
let thread_id = headers.get("X-GM-THRID").cloned();
```

**Why this fails:** `X-GM-THRID` is a Gmail IMAP extension attribute, not an email header. It appears in the IMAP FETCH response metadata, not in `RFC822.HEADER`.

### Proof of Concept: Thread IDs ARE Available

The proof script (`examples/thread_proof.py`) demonstrates successful extraction:

```
=== X-GM-THRID Extraction Results ===
✅ Successfully extracted: 50
❌ Failed to extract: 0

Raw IMAP response:
4037 (X-GM-THRID 1852322999435237597 X-GM-MSGID 1852322999435237597 UID 111405 ...)
```

The thread ID is present in the raw FETCH response—it just needs to be parsed correctly.

### Fix: Extract X-GM-THRID from Raw IMAP Response

#### Step 1: Add extraction function to `utils.rs`

```rust
// src/utils.rs - ADD THIS

use regex::Regex;

/// Extract Gmail Thread ID (X-GM-THRID) from an IMAP Fetch response.
///
/// Gmail provides X-GM-THRID as an IMAP extension attribute (not a header).
/// It must be parsed from the raw FETCH response.
///
/// Example response: "123 (X-GM-THRID 1852322999435237597 UID 45678 ...)"
pub fn extract_gmail_thread_id(fetch: &imap::types::Fetch) -> Option<String> {
    // The imap crate doesn't expose Gmail extensions directly,
    // so we parse from the debug representation
    let raw = format!("{:?}", fetch);

    // Look for X-GM-THRID followed by a number
    // Regex pattern: X-GM-THRID followed by space and digits
    lazy_static::lazy_static! {
        static ref THRID_RE: Regex = Regex::new(r"X-GM-THRID\s+(\d+)").unwrap();
    }

    THRID_RE.captures(&raw)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}

/// Extract Gmail Message ID (X-GM-MSGID) from an IMAP Fetch response.
pub fn extract_gmail_msg_id(fetch: &imap::types::Fetch) -> Option<String> {
    let raw = format!("{:?}", fetch);

    lazy_static::lazy_static! {
        static ref MSGID_RE: Regex = Regex::new(r"X-GM-MSGID\s+(\d+)").unwrap();
    }

    MSGID_RE.captures(&raw)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}
```

#### Step 2: Update Cargo.toml for lazy_static

```toml
[dependencies]
lazy_static = "1.4"
```

#### Step 3: Modify `imap_filter.rs` to extract thread ID during fetch

```rust
// src/imap_filter.rs - MODIFY fetch_messages()

fn fetch_messages(&mut self) -> Result<Vec<Message>> {
    // ... existing code ...

    for fetch in fetches.iter() {
        let uid = fetch.uid.unwrap_or(0);
        let seq = fetch.message;

        // Extract Gmail thread ID from raw response (THE FIX)
        let thread_id = crate::utils::extract_gmail_thread_id(fetch);

        // ... existing header parsing ...

        // Pass thread_id to Message::new()
        let msg = Message::new(
            uid,
            seq,
            raw_header,
            raw_labels,
            date_str,
            thread_id,  // NEW PARAMETER
        );

        // ... rest of loop ...
    }
}
```

#### Step 4: Update `Message::new()` signature

```rust
// src/message.rs - MODIFY

impl Message {
    pub fn new(
        uid: u32,
        seq: u32,
        raw_headers: Vec<u8>,
        raw_labels: Vec<String>,
        internal_date: String,
        gmail_thread_id: Option<String>,  // NEW PARAMETER
    ) -> Self {
        // ... existing parsing ...

        // Use the passed thread_id instead of trying to extract from headers
        Message {
            uid,
            seq,
            to,
            cc,
            from,
            subject,
            date: internal_date,
            labels,
            headers,
            message_id,
            in_reply_to,
            references,
            thread_id: gmail_thread_id,  // USE THE PARAMETER
        }
    }
}
```

---

## Thread-Aware State Transitions

### Design Goals

1. **If ANY message in a thread is protected (Starred/Important), protect the entire thread**
2. **TTL expiry should be based on the NEWEST message in the thread**
3. **When a thread expires, ALL messages in the thread transition together**

### Current `ThreadProcessor` Logic

The existing `ThreadProcessor` in `src/thread.rs` already has the structure for thread-aware processing:

```rust
pub fn process_thread_state_filter(
    &self,
    client: &mut Session<TlsStream<TcpStream>>,
    msg: &Message,
    filter: &StateFilter,
    action: &StateAction,
) -> Result<Vec<Message>> {
    // If message is part of a thread, evaluate TTL based on newest message
    if let Some(thread_id) = &msg.thread_id {
        if let Some(thread_msgs) = self.thread_map.get(thread_id) {
            // Only expire if ALL messages in thread have passed TTL
            let all_expired = thread_msgs.iter().all(|m| {
                filter.evaluate_ttl(m, chrono::Utc::now())
                    .map(|opt| opt.is_some())
                    .unwrap_or(false)
            });

            if all_expired {
                for thread_msg in thread_msgs {
                    apply_state_action(client, thread_msg, action)?;
                    processed.push(thread_msg.clone());
                }
            }
        }
    }
    // ...
}
```

**Issue:** This logic is correct, but it will never execute because `thread_id` is always `None` due to the extraction bug.

### After the Fix

Once `thread_id` is properly extracted, the existing `ThreadProcessor` will work correctly:

1. Messages are grouped by `X-GM-THRID`
2. When evaluating TTL, ALL messages in the thread are checked
3. Thread only expires when ALL messages have exceeded TTL
4. All messages in the thread are transitioned together

---

## Configuration Schema Reference

### Top-Level Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `imap-domain` | string | Yes* | IMAP server hostname |
| `imap-username` | string | Yes* | IMAP login username |
| `imap-password` | string | No | IMAP password (prefer env var) |
| `message-filters` | list | No | List of MessageFilter definitions |
| `state-filters` | list | No | List of StateFilter definitions |

*Can be provided via CLI args or environment variables instead.

### MessageFilter Schema

```yaml
- <filter-name>:
    to: <address-filter>       # Optional: To recipients
    cc: <address-filter>       # Optional: CC recipients
    from: <address-filter>     # Optional: Sender
    subject: [<glob>, ...]     # Optional: Subject patterns
    label: <label>             # Optional: Single label (alias for labels)
    labels:                    # Optional: Label requirements
      included: [<label>, ...]
      excluded: [<label>, ...]
    action: <action>           # Required: Action to perform
    actions: [<action>, ...]   # Alternative: Multiple actions
```

**Address Filter Formats:**
- String: `'*@domain.com'` - Single glob pattern
- List: `['a@x.com', 'b@x.com']` - Multiple patterns (OR logic)
- Empty list: `[]` - Require field to be empty

**Actions:**
- `Star` - Add Starred label
- `Flag` - Add Important label
- `<label-name>` - Move to label

### StateFilter Schema

```yaml
- <filter-name>:
    label: <label>             # Single label (alias)
    labels: [<label>, ...]     # Messages must have any of these
    ttl: <ttl-spec>            # Required: Time-to-live specification
    action: <state-action>     # Action when TTL expires
    nerf: bool                 # If true, log but don't apply action
```

**TTL Formats:**
- `Keep` - Never expire
- `<n>d` - Expire after n days
- `{ read: <n>d, unread: <m>d }` - Different TTL based on read state

**State Actions:**
- `<label-name>` - Move to label (string shorthand)
- `Move: <label-name>` - Move to label (explicit)
- `Delete` - Mark as deleted

---

## Execution Flow

### Startup

```
1. Parse CLI arguments (clap)
2. Load YAML configuration
3. Merge CLI/env overrides with config
4. Establish TLS connection to IMAP server
5. Authenticate with username/password
6. Create IMAPFilter instance
```

### Filter Execution

```
1. SELECT INBOX
2. SEARCH ALL → get message sequence numbers
3. FETCH (UID FLAGS INTERNALDATE X-GM-THRID RFC822.HEADER) for all messages
4. Parse each fetch response into Message structs
5. Build thread map: HashMap<ThreadId, Vec<Message>>

Phase 1: MessageFilters
├── For each message:
│   ├── Check if any MessageFilter matches
│   ├── If match: apply action to message (and thread if applicable)
│   └── Remove processed messages from list

Phase 2: StateFilters
├── For each remaining message:
│   ├── Find first matching StateFilter
│   ├── If TTL == Keep: remove from list (protected)
│   ├── If TTL expired: apply action to message (and thread)
│   └── Remove processed messages from list

6. LOGOUT
```

---

## Known Issues & Technical Debt

### Critical

1. **X-GM-THRID extraction broken** (documented fix above)
2. **Bug in `imap_filter.rs`**: Uses wrong filter index
   ```rust
   // Line 215: Uses `i` but should use the matched filter
   &self.message_filters[i]  // BUG: `i` is message index, not filter index
   ```

### Medium

3. **TTL read/unread distinction not implemented**
   ```rust
   // state_filter.rs:142 - Always uses `unread` duration
   TTL::Detailed { unread, .. } => *unread,
   ```

4. **Unused parameter in `ThreadProcessor::process_thread_message_filter`**
   - `filter: &MessageFilter` parameter is never used

### Low

5. **No mailing list detection** for "only to me" enhancement
6. **No `to_exact` mode** for requiring sole recipient
7. **Header parsing doesn't handle multi-line headers** (RFC 2822 folding)

---

## Future Enhancements

### "Only to Me" Filter Improvements

```yaml
message-filters:
  - direct-only:
      to: ['me@domain.com']
      to_exact: true           # NEW: Must be sole To recipient
      cc: []
      no_mailing_list: true    # NEW: Reject List-Id headers
      action: Star
```

Implementation:
- Add `to_exact: bool` field to MessageFilter
- Add `no_mailing_list: bool` field
- Check headers: `List-Id`, `List-Unsubscribe`, `Precedence: bulk/list`

### Thread Protection Modes

```yaml
state-filters:
  - Protected:
      labels: [Starred]
      ttl: Keep
      thread_mode: protect_all  # NEW: If any message starred, protect thread
```

Options:
- `protect_all`: Any protected message protects entire thread
- `independent`: Each message evaluated independently (current default)
- `newest_wins`: Use newest message's state for entire thread

### OAuth2 Authentication

Replace password-based auth with OAuth2 for better security:

```yaml
auth:
  type: oauth2
  client_id: "..."
  client_secret: "..."
  token_file: ~/.config/imap-filter/oauth-token.json
```

---

## Testing

### Unit Tests

- `cfg/message_filter.rs`: Pattern matching logic
- `cfg/state_filter.rs`: TTL evaluation
- `message.rs`: Header parsing
- `utils.rs`: Gmail extension extraction

### Integration Tests

- `examples/thread_proof.py`: Validates X-GM-THRID extraction
- `examples/thread_proof.rs`: Same test in Rust

### Manual Testing

```bash
# Run with debug logging
IMAP_PASSWORD=$(cat ~/.config/imap-filter/imap-filter.creds) \
  cargo run -- --debug

# Check log output
tail -f imap-filter.log
```

---

## Appendix: Gmail IMAP Extensions

Gmail provides several IMAP extensions beyond RFC 3501:

| Extension | Description | Used By |
|-----------|-------------|---------|
| `X-GM-THRID` | Gmail thread ID (conversation grouping) | Thread support |
| `X-GM-MSGID` | Gmail message ID | Deduplication |
| `X-GM-LABELS` | Gmail labels on message | Label filtering |
| `X-GM-RAW` | Raw Gmail search syntax | Advanced search |

These are fetched as IMAP attributes, not email headers:

```
FETCH 1:* (UID X-GM-THRID X-GM-MSGID X-GM-LABELS RFC822.HEADER)
```

Response format:
```
* 1 FETCH (UID 12345 X-GM-THRID 1852322999435237597 X-GM-MSGID 1852322999435237598 X-GM-LABELS ("INBOX" "Important") ...)
```

