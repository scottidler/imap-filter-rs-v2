//! Quick test of thread grouping from standard headers (Message-ID/References/In-Reply-To)
//!
//! Run with:
//! ```
//! IMAP_USERNAME=your@email.com IMAP_PASSWORD=your-app-password cargo run --example thread_test
//! ```

use std::collections::HashMap;
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let domain = env::var("IMAP_DOMAIN").unwrap_or_else(|_| "imap.gmail.com".to_string());
    let username = env::var("IMAP_USERNAME").expect("IMAP_USERNAME env var required");
    let password = env::var("IMAP_PASSWORD").expect("IMAP_PASSWORD env var required");
    let limit: usize = env::var("LIMIT").ok().and_then(|s| s.parse().ok()).unwrap_or(100);

    println!("=== Thread Grouping Test (Standard Headers) ===\n");
    println!("Connecting to {}...", domain);

    let client = imap::ClientBuilder::new(&domain, 993).connect()?;
    let mut session = client.login(&username, &password).map_err(|e| e.0)?;

    println!("✅ Logged in as {}", username);

    let mailbox = session.select("INBOX")?;
    println!("📬 Selected 'INBOX' - {} messages\n", mailbox.exists);

    // Get recent messages (last N by sequence number)
    let total = mailbox.exists as usize;
    let start = if total > limit { total - limit + 1 } else { 1 };
    let seq_range = format!("{}:{}", start, total);

    println!("Fetching messages {} (limit: {})...", seq_range, limit);

    // Fetch without X-GM-THRID - just standard headers
    let fetches = session.fetch(&seq_range, "(UID RFC822.HEADER)")?;
    println!("✅ Fetched {} messages\n", fetches.len());

    // Parse threading headers
    struct MsgInfo {
        uid: u32,
        subject: String,
        message_id: Option<String>,
        in_reply_to: Option<String>,
        references: Vec<String>,
    }

    let mut messages: Vec<MsgInfo> = Vec::new();

    for fetch in fetches.iter() {
        let uid = fetch.uid.unwrap_or(0);
        let header_bytes = fetch.header().unwrap_or(&[]);
        let header_text = String::from_utf8_lossy(header_bytes);

        // Simple header parsing
        let mut headers: HashMap<String, String> = HashMap::new();
        let mut current_key = String::new();
        let mut current_value = String::new();

        for line in header_text.lines() {
            if line.starts_with(' ') || line.starts_with('\t') {
                // Continuation of previous header
                current_value.push(' ');
                current_value.push_str(line.trim());
            } else if let Some(colon_pos) = line.find(':') {
                // Save previous header
                if !current_key.is_empty() {
                    headers.insert(current_key.clone(), current_value.trim().to_string());
                }
                current_key = line[..colon_pos].to_string();
                current_value = line[colon_pos + 1..].to_string();
            }
        }
        // Save last header
        if !current_key.is_empty() {
            headers.insert(current_key, current_value.trim().to_string());
        }

        let subject = headers.get("Subject").cloned().unwrap_or_default();
        let message_id = headers.get("Message-ID").or(headers.get("Message-Id")).cloned();
        let in_reply_to = headers.get("In-Reply-To").or(headers.get("In-reply-to")).cloned();
        let references: Vec<String> = headers
            .get("References")
            .map(|r| r.split_whitespace().map(String::from).collect())
            .unwrap_or_default();

        messages.push(MsgInfo {
            uid,
            subject,
            message_id,
            in_reply_to,
            references,
        });
    }

    // Build thread groups using Union-Find style approach
    // Key insight: messages in the same thread share Message-IDs in their References chain

    // Step 1: Map each message_id to its UID
    let mut msgid_to_uid: HashMap<String, u32> = HashMap::new();
    for msg in &messages {
        if let Some(ref mid) = msg.message_id {
            msgid_to_uid.insert(mid.clone(), msg.uid);
        }
    }

    // Step 2: Build thread groups - use the first referenced message_id as thread root
    let mut thread_groups: HashMap<String, Vec<u32>> = HashMap::new();

    for msg in &messages {
        // Determine thread root: first message in references chain, or own message_id
        let thread_root = msg
            .references
            .first()
            .or(msg.in_reply_to.as_ref())
            .or(msg.message_id.as_ref())
            .cloned()
            .unwrap_or_else(|| format!("orphan-{}", msg.uid));

        thread_groups.entry(thread_root).or_default().push(msg.uid);
    }

    // Stats
    let multi_msg_threads: Vec<_> = thread_groups.iter().filter(|(_, uids)| uids.len() > 1).collect();

    println!("=== Thread Analysis ===");
    println!("Total messages: {}", messages.len());
    println!("Total threads: {}", thread_groups.len());
    println!("Multi-message threads: {}", multi_msg_threads.len());

    // Show some multi-message threads
    println!("\n=== Sample Multi-Message Threads ===");
    for (thread_id, uids) in multi_msg_threads.iter().take(5) {
        println!("\n📧 Thread (root: {}...)", &thread_id[..thread_id.len().min(50)]);
        println!("   {} messages in thread", uids.len());
        for uid in uids.iter().take(3) {
            if let Some(msg) = messages.iter().find(|m| m.uid == *uid) {
                let subj = if msg.subject.len() > 60 {
                    format!("{}...", &msg.subject[..60])
                } else {
                    msg.subject.clone()
                };
                println!("   - UID {}: {}", uid, subj);
            }
        }
        if uids.len() > 3 {
            println!("   ... and {} more", uids.len() - 3);
        }
    }

    session.logout()?;
    println!("\n✅ Done!");
    Ok(())
}
