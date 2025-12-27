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

## Design Philosophy

### COMPOSABLE PRIMITIVES, NOT SPECIAL-CASE FIELDS

**CRITICAL: The filter system MUST be built from generic, composable primitives.**

❌ **WRONG** - Adding special-case boolean fields:
```yaml
message-filters:
  - bad-filter:
      to: ['me@example.com']
      to_exact: true           # NO! Special-case field
      only_to_me: true         # NO! Special-case field
      no_mailing_list: true    # NO! Special-case field
```

✅ **RIGHT** - Using composable primitives:
```yaml
message-filters:
  - only-to-me:                # Filter NAME describes intent
      to: ['me@example.com']   # Pattern matching
      cc: []                   # Empty = require empty
      headers:                 # Generic header matching
        List-Id: []            # Reject if List-Id exists (empty pattern = must not exist)
```

The filter is NAMED "only-to-me" but BUILT from generic primitives that can be combined in any way.

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

## Filter Primitives

### AddressFilter Primitive

Used for `to`, `cc`, `bcc`, `from` fields.

| Syntax | Meaning |
|--------|---------|
| `['pattern']` | Field contains address matching pattern (glob) |
| `['p1', 'p2']` | Field contains address matching ANY pattern |
| `[]` | Field must be EMPTY (no recipients) |

### Header Matching Primitive

Generic header pattern matching via `headers` field.

| Syntax | Meaning |
|--------|---------|
| `headers: { "X-Priority": ["1"] }` | Header must exist and match pattern |
| `headers: { "List-Id": [] }` | Header must NOT exist (reject if present) |
| `headers: { "List-Id": ["*"] }` | Header must exist (any value) |

---

## Implementation Phases

### Phase 1: Core MessageFilters ✅ COMPLETE

Basic message filtering with pattern matching.

**Implemented:**
- `to`, `cc`, `from` address pattern matching
- `subject` glob patterns
- `labels` include/exclude filtering
- `headers` custom header pattern matching
- Actions: Star, Flag, Move

### Phase 2: Thread Support ✅ COMPLETE

Thread-aware filtering using Gmail X-GM-THRID and standard headers.

**Implemented:**
- Gmail X-GM-THRID extraction from FETCH response
- Standard IMAP thread grouping (Message-ID, In-Reply-To, References)
- Thread-aware message filter actions (apply to entire thread)
- Thread-aware state filter TTL (newest message determines thread expiry)

### Phase 3: OAuth2 Authentication ✅ COMPLETE

OAuth2 authentication as an additional method alongside password/app-password.

**Implemented:**
- OAuth2 credentials config: `oauth2-client-id`, `oauth2-client-secret`, `oauth2-refresh-token`
- CLI/env support: `--oauth2-client-id`, `OAUTH2_CLIENT_ID`, etc.
- XOAUTH2 IMAP authentication mechanism
- Automatic token refresh via Google's token endpoint
- Falls back to password auth if OAuth2 credentials not provided

---

## Filter Types

### 1. MessageFilters

MessageFilters match incoming messages based on headers and apply immediate actions.

**Primitives:**
- `to`: AddressFilter for To recipients
- `cc`: AddressFilter for CC recipients
- `from`: AddressFilter for sender
- `subject`: List of glob patterns
- `labels`: Include/exclude label filters
- `headers`: Custom header pattern matching

**Actions:**
- `Star`: Add `\Starred` flag
- `Flag`: Add `\Important` flag
- `Move`: Move to label/folder

**Example filters built from primitives:**
```yaml
message-filters:
  # "Only to me" - direct email with no CC
  - direct-messages:
      to: ['me@example.com']
      cc: []                   # Require no CC
      headers:
        List-Id: []            # Must NOT be from a mailing list
      action: Star

  # High priority messages - using header matching
  - urgent:
      headers:
        X-Priority: ["1", "2"]
      action: Flag

  # GitHub notifications - using header matching
  - github:
      headers:
        List-Id: ["*github*"]
      action:
        Move: GitHub

  # NOT from mailing lists - using header rejection
  - personal-only:
      to: ['me@example.com']
      headers:
        List-Id: []           # Must NOT have List-Id
        List-Unsubscribe: []  # Must NOT have List-Unsubscribe
      action: Star
```

### 2. StateFilters

StateFilters evaluate message state (labels, age) and apply time-based transitions.

**Primitives:**
- `labels`: Messages must have any of these labels
- `ttl`: Time-to-live specification

**Thread Protection:** Automatic. If ANY message in a thread matches a protective state (`ttl: Keep`), the entire thread is protected. No configuration needed - calculated dynamically at evaluation time.

**TTL Types:**
- `Keep`: Never expire (protect from all expiry)
- `<n>d`: Expire after n days
- `{ read: <n>d, unread: <m>d }`: Different TTL for read vs unread

**Actions:**
- `Move`: Move to destination label
- `Delete`: Mark as deleted

```yaml
state-filters:
  - Starred:
      labels: [Important, Starred]
      ttl: Keep

  - Cull:
      ttl:
        read: 7d
        unread: 21d
      action: Purgatory

  - Purge:
      label: Purgatory
      ttl: 3d
      action:
        Move: Oblivion
```

---

## Thread Support

### Gmail Thread Grouping (X-GM-THRID)

Gmail provides thread IDs via the `X-GM-THRID` IMAP extension attribute.

**Extraction:** Parse from raw FETCH response using regex:
```rust
pub fn extract_gmail_thread_id(fetch: &imap::types::Fetch) -> Option<String> {
    let raw = format!("{:?}", fetch);
    let re = Regex::new(r"X-GM-THRID\s+(\d+)").ok()?;
    re.captures(&raw)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}
```

### Standard IMAP Thread Grouping

For non-Gmail servers, use standard RFC headers:
- `Message-ID`: Unique message identifier
- `In-Reply-To`: Parent message ID
- `References`: Chain of ancestor message IDs

Thread map is built using graph traversal (BFS) to find connected components.

---

## Configuration Schema

### Top-Level Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `imap-domain` | string | Yes* | IMAP server hostname |
| `imap-username` | string | Yes* | IMAP login username |
| `imap-password` | string | No | IMAP password (prefer env var) |
| `message-filters` | list | No | List of MessageFilter definitions |
| `state-filters` | list | No | List of StateFilter definitions |

### MessageFilter Schema

```yaml
- <filter-name>:
    to: <address-filter>       # Optional
    cc: <address-filter>       # Optional
    from: <address-filter>     # Optional
    subject: [<glob>, ...]     # Optional
    labels:                    # Optional
      included: [<label>, ...]
      excluded: [<label>, ...]
    headers:                   # Optional
      <header-name>: [<pattern>, ...]
    action: <action>           # Required
```

### StateFilter Schema

```yaml
- <filter-name>:
    labels: [<label>, ...]     # Messages must have any of these
    ttl: <ttl-spec>            # Required
    action: <state-action>     # Action when TTL expires
```

---

## Testing

### Unit Tests

- `cfg/message_filter.rs`: Pattern matching, address filters
- `cfg/state_filter.rs`: TTL evaluation
- `message.rs`: Header parsing
- `thread.rs`: Thread grouping (Gmail and standard)
- `utils.rs`: Gmail extension extraction

### Running Tests

```bash
cargo test
otto ci
```

---

## Appendix: Gmail IMAP Extensions

| Extension | Description | Used By |
|-----------|-------------|---------|
| `X-GM-THRID` | Gmail thread ID | Thread support |
| `X-GM-MSGID` | Gmail message ID | Deduplication |
| `X-GM-LABELS` | Gmail labels | Label filtering |
| `X-GM-RAW` | Raw Gmail search | Advanced search |
