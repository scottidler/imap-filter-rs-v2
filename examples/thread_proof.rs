// examples/thread_proof.rs
//
// Proof-of-concept: Demonstrate that Gmail X-GM-THRID can be extracted
// and used for thread grouping via IMAP.
//
// Run with:
//   IMAP_USERNAME=you@gmail.com IMAP_PASSWORD=your-app-password cargo run --example thread_proof
//
// Or with a specific mailbox:
//   IMAP_MAILBOX=INBOX cargo run --example thread_proof

use std::collections::HashMap;
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get credentials from environment
    let domain = env::var("IMAP_DOMAIN").unwrap_or_else(|_| "imap.gmail.com".to_string());
    let username = env::var("IMAP_USERNAME").expect("IMAP_USERNAME env var required");
    let password = env::var("IMAP_PASSWORD").expect("IMAP_PASSWORD env var required");
    let mailbox = env::var("IMAP_MAILBOX").unwrap_or_else(|_| "INBOX".to_string());
    let limit: usize = env::var("LIMIT")
        .unwrap_or_else(|_| "50".to_string())
        .parse()
        .unwrap_or(50);

    println!("=== Gmail Thread ID (X-GM-THRID) Proof of Concept ===\n");
    println!("Connecting to {}...", domain);

    // Connect with TLS
    let tls = native_tls::TlsConnector::builder().build()?;
    let client = imap::connect((domain.as_str(), 993), &domain, &tls)?;

    // Login
    let mut session = client.login(&username, &password).map_err(|(e, _)| e)?;

    println!("✅ Logged in as {}", username);

    // Select mailbox
    let mailbox_info = session.select(&mailbox)?;
    println!("📬 Selected '{}' - {} messages\n", mailbox, mailbox_info.exists);

    if mailbox_info.exists == 0 {
        println!("No messages in mailbox.");
        session.logout()?;
        return Ok(());
    }

    // Fetch recent messages with Gmail extensions
    let range = if mailbox_info.exists > limit as u32 {
        format!("{}:{}", mailbox_info.exists - limit as u32 + 1, mailbox_info.exists)
    } else {
        "1:*".to_string()
    };

    println!("Fetching messages {} with X-GM-THRID...\n", range);

    // Key: We request X-GM-THRID as part of the FETCH
    let fetches = session.fetch(
        &range,
        "(UID X-GM-THRID X-GM-MSGID BODY.PEEK[HEADER.FIELDS (SUBJECT FROM DATE MESSAGE-ID)])",
    )?;

    // Structure to hold message info
    #[derive(Debug, Clone)]
    struct MessageInfo {
        uid: u32,
        thread_id: Option<String>,
        subject: String,
        from: String,
        date: String,
    }

    let mut messages: Vec<MessageInfo> = Vec::new();
    let mut thread_extraction_success = 0;
    let mut thread_extraction_failed = 0;

    for fetch in fetches.iter() {
        let uid = fetch.uid.unwrap_or(0);

        // Get the raw debug representation to extract Gmail extensions
        let raw = format!("{:?}", fetch);

        // Extract X-GM-THRID
        let thread_id = extract_field(&raw, "X-GM-THRID ");

        if thread_id.is_some() {
            thread_extraction_success += 1;
        } else {
            thread_extraction_failed += 1;
        }

        // Parse headers
        let header_bytes = fetch.header().unwrap_or(&[]);
        let header_str = String::from_utf8_lossy(header_bytes);

        let subject = extract_header(&header_str, "Subject:");
        let from = extract_header(&header_str, "From:");
        let date = extract_header(&header_str, "Date:");

        messages.push(MessageInfo {
            uid,
            thread_id,
            subject,
            from,
            date,
        });
    }

    // Report extraction results
    println!("=== X-GM-THRID Extraction Results ===");
    println!("✅ Successfully extracted: {}", thread_extraction_success);
    println!("❌ Failed to extract: {}", thread_extraction_failed);
    println!();

    if thread_extraction_success == 0 {
        println!("⚠️  WARNING: No thread IDs were extracted!");
        println!("   This might mean:");
        println!("   1. Gmail extensions aren't enabled for this account");
        println!("   2. The parsing logic needs adjustment");
        println!();

        // Show a sample raw response for debugging
        if let Some(fetch) = fetches.iter().next() {
            println!("Sample raw FETCH response (for debugging):");
            println!("{:?}", fetch);
        }
    } else {
        // Group by thread ID
        let mut threads: HashMap<String, Vec<&MessageInfo>> = HashMap::new();
        let mut no_thread: Vec<&MessageInfo> = Vec::new();

        for msg in &messages {
            if let Some(ref tid) = msg.thread_id {
                threads.entry(tid.clone()).or_default().push(msg);
            } else {
                no_thread.push(msg);
            }
        }

        // Find threads with multiple messages (the interesting case for state-filters)
        let multi_message_threads: Vec<_> = threads.iter().filter(|(_, msgs)| msgs.len() > 1).collect();

        println!("=== Thread Grouping Results ===");
        println!("Total unique threads: {}", threads.len());
        println!("Threads with multiple messages: {}", multi_message_threads.len());
        println!("Messages without thread ID: {}", no_thread.len());
        println!();

        // Show some example multi-message threads
        if !multi_message_threads.is_empty() {
            println!("=== Example Multi-Message Threads ===");
            println!("(This proves thread grouping works for state-filter transitions)\n");

            for (i, (thread_id, msgs)) in multi_message_threads.iter().take(3).enumerate() {
                println!("Thread #{} (ID: {})", i + 1, thread_id);
                println!("  Messages in thread: {}", msgs.len());

                for (j, msg) in msgs.iter().enumerate() {
                    println!("  [{}] UID: {}", j + 1, msg.uid);
                    println!("      Subject: {}", truncate(&msg.subject, 60));
                    println!("      From: {}", truncate(&msg.from, 40));
                    println!("      Date: {}", msg.date);
                }
                println!();
            }

            println!("=== PROOF: State-Filter Thread Transitions Are Achievable ===");
            println!();
            println!("Since we can:");
            println!("  1. ✅ Extract X-GM-THRID from Gmail IMAP responses");
            println!("  2. ✅ Group messages by thread ID");
            println!("  3. ✅ Identify all messages belonging to the same conversation");
            println!();
            println!("We CAN implement state-filter transitions that operate on entire threads:");
            println!("  - If ANY message in a thread is Starred → protect the whole thread");
            println!("  - If the NEWEST message in a thread expires → expire the whole thread");
            println!("  - Apply TTL based on the most recent message in the thread");
            println!();
        } else {
            println!("No multi-message threads found in the sample.");
            println!("Try with a larger LIMIT or a different mailbox with conversation threads.");
        }
    }

    // Logout
    session.logout()?;
    println!("✅ Done.");

    Ok(())
}

/// Extract a Gmail extension field from the raw FETCH debug output
fn extract_field(raw: &str, prefix: &str) -> Option<String> {
    if let Some(start) = raw.find(prefix) {
        let rest = &raw[start + prefix.len()..];
        // The value is a number, ends at space, comma, or other delimiter
        let value: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

/// Extract a header value from raw header text
fn extract_header(headers: &str, name: &str) -> String {
    for line in headers.lines() {
        if line.to_lowercase().starts_with(&name.to_lowercase()) {
            return line[name.len()..].trim().to_string();
        }
    }
    String::new()
}

/// Truncate a string to max length
fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max])
    } else {
        s.to_string()
    }
}
