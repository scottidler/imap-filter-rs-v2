//! Test if X-GM-LABELS can be included in a batch FETCH using imap v3
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let domain = env::var("IMAP_DOMAIN").unwrap_or_else(|_| "imap.gmail.com".to_string());
    let username = env::var("IMAP_USERNAME").expect("IMAP_USERNAME required");
    let password = env::var("IMAP_PASSWORD").expect("IMAP_PASSWORD required");

    println!("Testing imap v3 Gmail extension support...\n");

    let client = imap::ClientBuilder::new(&domain, 993).connect()?;
    let mut session = client.login(&username, &password).map_err(|e| e.0)?;

    let mbox = session.select("INBOX")?;
    println!("Mailbox has {} messages\n", mbox.exists);

    // Test 1: Fetch with X-GM-THRID FIRST (before any other queries)
    println!("=== Test 1: Fetching with X-GM-THRID ===");
    match session.fetch("1:3", "(UID X-GM-THRID)") {
        Ok(fetches) => {
            println!("✅ Batch fetch with X-GM-THRID worked! Got {} results", fetches.len());
            for f in fetches.iter() {
                println!("UID: {:?}, Seq: {}", f.uid, f.message);
            }
        }
        Err(e) => println!("❌ Batch fetch with X-GM-THRID failed: {:?}", e),
    }

    // Test 2: Fetch with X-GM-LABELS
    println!("\n=== Test 2: Fetching with X-GM-LABELS ===");
    match session.fetch("1:5", "(UID FLAGS X-GM-LABELS)") {
        Ok(fetches) => {
            println!(
                "✅ Batch fetch with X-GM-LABELS worked! Got {} results\n",
                fetches.len()
            );
            for f in fetches.iter().take(3) {
                println!("UID: {:?}", f.uid);
                println!("Flags: {:?}", f.flags());
                // Use v3's gmail_labels() accessor
                if let Some(labels) = f.gmail_labels() {
                    let labels: Vec<&str> = labels.collect();
                    println!("Gmail Labels (via accessor): {:?}", labels);
                } else {
                    println!("Gmail Labels: None found");
                }
                println!();
            }
        }
        Err(e) => println!("❌ Batch fetch with X-GM-LABELS failed: {:?}", e),
    }

    // Test 3: Combined fetch (what failed in v2.x)
    println!("\n=== Test 3: Combined fetch ===");
    match session.fetch("1:5", "(UID FLAGS X-GM-LABELS INTERNALDATE RFC822.HEADER)") {
        Ok(fetches) => {
            println!("✅ Combined fetch works! Got {} results", fetches.len());
            for f in fetches.iter().take(2) {
                println!("UID {:?}: {:?}", f.uid, f.gmail_labels().map(|l| l.collect::<Vec<_>>()));
            }
        }
        Err(e) => println!("❌ Combined fetch failed: {:?}", e),
    }

    session.logout()?;
    Ok(())
}
