// tests/harness/fixtures.rs
//
// Email fixture loader for loading .eml files from the test fixtures directory.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::harness::virtual_mailbox::MailboxMessage;

/// Represents a loaded email fixture with metadata.
#[derive(Debug, Clone)]
pub struct EmailFixture {
    /// The parsed mailbox message
    pub message: MailboxMessage,
    /// Original file path for debugging
    pub source_path: String,
}

/// Error type for fixture loading operations.
#[derive(Debug)]
pub enum FixtureError {
    Io(std::io::Error),
    Parse(String),
    MissingHeader(String),
}

impl std::fmt::Display for FixtureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FixtureError::Io(e) => write!(f, "IO error: {}", e),
            FixtureError::Parse(msg) => write!(f, "Parse error: {}", msg),
            FixtureError::MissingHeader(header) => write!(f, "Missing required header: {}", header),
        }
    }
}

impl std::error::Error for FixtureError {}

impl From<std::io::Error> for FixtureError {
    fn from(err: std::io::Error) -> Self {
        FixtureError::Io(err)
    }
}

/// Loader for email fixtures from .eml files.
pub struct FixtureLoader {
    base_path: PathBuf,
}

impl FixtureLoader {
    /// Create a new fixture loader pointing to the standard fixtures directory.
    pub fn new() -> Self {
        Self {
            base_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("emails"),
        }
    }

    /// Create a fixture loader with a custom base path.
    pub fn with_base_path(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Load a single .eml file into a MailboxMessage.
    pub fn load_email(&self, relative_path: &str) -> Result<EmailFixture, FixtureError> {
        let path = self.base_path.join(relative_path);
        let content = std::fs::read_to_string(&path)?;

        let message = parse_eml(&content)?;

        Ok(EmailFixture {
            message,
            source_path: path.to_string_lossy().to_string(),
        })
    }

    /// Load all .eml files from a directory.
    pub fn load_directory(&self, relative_path: &str) -> Result<Vec<EmailFixture>, FixtureError> {
        let dir = self.base_path.join(relative_path);
        let mut fixtures = Vec::new();

        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map(|e| e == "eml").unwrap_or(false) {
                let relative = path
                    .strip_prefix(&self.base_path)
                    .map_err(|e| FixtureError::Parse(e.to_string()))?
                    .to_string_lossy()
                    .to_string();

                fixtures.push(self.load_email(&relative)?);
            }
        }

        // Sort by filename for predictable ordering
        fixtures.sort_by(|a, b| a.source_path.cmp(&b.source_path));
        Ok(fixtures)
    }

    /// Get the base path for fixtures.
    // TEMPORARY: Will be used in Phase 2+ for scenario loading
    #[allow(dead_code)]
    pub fn base_path(&self) -> &PathBuf {
        &self.base_path
    }
}

impl Default for FixtureLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse an .eml file content into a MailboxMessage.
fn parse_eml(content: &str) -> Result<MailboxMessage, FixtureError> {
    let mut headers: HashMap<String, String> = HashMap::new();
    let mut in_headers = true;

    for line in content.lines() {
        if in_headers {
            if line.is_empty() {
                in_headers = false;
                continue;
            }

            // Handle header continuation (lines starting with whitespace)
            if line.starts_with(' ') || line.starts_with('\t') {
                // Continuation of previous header - skip for simplicity
                continue;
            }

            if let Some((key, value)) = line.split_once(": ") {
                headers.insert(key.to_string(), value.to_string());
            } else if let Some((key, value)) = line.split_once(':') {
                headers.insert(key.to_string(), value.trim().to_string());
            }
        }
    }

    // Extract required fields
    let from = headers
        .get("From")
        .ok_or_else(|| FixtureError::MissingHeader("From".to_string()))?
        .clone();

    let to = headers.get("To").cloned().unwrap_or_default();

    let subject = headers.get("Subject").cloned().unwrap_or_default();

    let date = headers
        .get("Date")
        .cloned()
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    // Build the message
    let mut message = MailboxMessage::new(0, &subject, &extract_email(&from), &extract_email(&to), &date);

    // Set optional threading headers
    if let Some(message_id) = headers.get("Message-ID") {
        message = message.with_message_id(message_id);
    }

    if let Some(in_reply_to) = headers.get("In-Reply-To") {
        message = message.with_in_reply_to(in_reply_to);
    }

    if let Some(references) = headers.get("References") {
        let refs: Vec<&str> = references.split_whitespace().collect();
        message = message.with_references(&refs);
    }

    // Set CC if present
    if let Some(cc) = headers.get("Cc") {
        let cc_addrs: Vec<&str> = cc.split(',').map(|s| s.trim()).collect();
        let cc_emails: Vec<&str> = cc_addrs.iter().map(|a| extract_email_str(a)).collect();
        message = message.with_cc(&cc_emails);
    }

    // Store all headers for custom header matching
    for (key, value) in &headers {
        message = message.with_header(key, value);
    }

    Ok(message)
}

/// Extract just the email address from a header value like "Name <email@example.com>".
fn extract_email(header_value: &str) -> String {
    extract_email_str(header_value).to_string()
}

/// Extract just the email address from a header value (returns &str).
fn extract_email_str(header_value: &str) -> &str {
    if let Some(start) = header_value.find('<') {
        if let Some(end) = header_value.find('>') {
            return &header_value[start + 1..end];
        }
    }
    header_value.trim()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_eml(dir: &std::path::Path, filename: &str, content: &str) {
        let path = dir.join(filename);
        fs::write(path, content).unwrap();
    }

    #[test]
    fn test_parse_simple_eml() {
        let content = r#"From: sender@example.com
To: recipient@example.com
Subject: Test Subject
Date: Mon, 1 Jan 2024 10:00:00 +0000
Message-ID: <test-001@example.com>

This is the body.
"#;

        let message = parse_eml(content).unwrap();
        assert_eq!(message.subject, "Test Subject");
        assert_eq!(message.from, vec!["sender@example.com"]);
        assert_eq!(message.to, vec!["recipient@example.com"]);
        assert_eq!(message.message_id, Some("<test-001@example.com>".to_string()));
    }

    #[test]
    fn test_parse_eml_with_display_names() {
        let content = r#"From: John Doe <john@example.com>
To: Jane Smith <jane@example.com>
Subject: Hello
Date: Mon, 1 Jan 2024 10:00:00 +0000

Body
"#;

        let message = parse_eml(content).unwrap();
        assert_eq!(message.from, vec!["john@example.com"]);
        assert_eq!(message.to, vec!["jane@example.com"]);
    }

    #[test]
    fn test_parse_eml_with_cc() {
        let content = r#"From: sender@example.com
To: recipient@example.com
Cc: cc1@example.com, cc2@example.com
Subject: With CC
Date: Mon, 1 Jan 2024 10:00:00 +0000

Body
"#;

        let message = parse_eml(content).unwrap();
        assert_eq!(message.cc.len(), 2);
        assert!(message.cc.contains(&"cc1@example.com".to_string()));
        assert!(message.cc.contains(&"cc2@example.com".to_string()));
    }

    #[test]
    fn test_parse_eml_with_threading_headers() {
        let content = r#"From: sender@example.com
To: recipient@example.com
Subject: Reply
Date: Mon, 1 Jan 2024 10:00:00 +0000
Message-ID: <reply@example.com>
In-Reply-To: <original@example.com>
References: <root@example.com> <original@example.com>

Body
"#;

        let message = parse_eml(content).unwrap();
        assert_eq!(message.message_id, Some("<reply@example.com>".to_string()));
        assert_eq!(message.in_reply_to, Some("<original@example.com>".to_string()));
        assert_eq!(message.references.len(), 2);
    }

    #[test]
    fn test_parse_eml_missing_from_fails() {
        let content = r#"To: recipient@example.com
Subject: No From
Date: Mon, 1 Jan 2024 10:00:00 +0000

Body
"#;

        let result = parse_eml(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_fixture_loader_load_email() {
        let temp_dir = TempDir::new().unwrap();
        let emails_dir = temp_dir.path().join("simple");
        fs::create_dir_all(&emails_dir).unwrap();

        create_test_eml(
            &emails_dir,
            "test.eml",
            r#"From: test@example.com
To: recipient@example.com
Subject: Fixture Test
Date: Mon, 1 Jan 2024 10:00:00 +0000

Body
"#,
        );

        let loader = FixtureLoader::with_base_path(temp_dir.path().to_path_buf());
        let fixture = loader.load_email("simple/test.eml").unwrap();

        assert_eq!(fixture.message.subject, "Fixture Test");
        assert!(fixture.source_path.contains("test.eml"));
    }

    #[test]
    fn test_fixture_loader_load_directory() {
        let temp_dir = TempDir::new().unwrap();
        let thread_dir = temp_dir.path().join("thread-01");
        fs::create_dir_all(&thread_dir).unwrap();

        create_test_eml(
            &thread_dir,
            "01-initial.eml",
            r#"From: alice@example.com
To: bob@example.com
Subject: Initial
Date: Mon, 1 Jan 2024 10:00:00 +0000
Message-ID: <msg1@example.com>

Body
"#,
        );

        create_test_eml(
            &thread_dir,
            "02-reply.eml",
            r#"From: bob@example.com
To: alice@example.com
Subject: Re: Initial
Date: Mon, 1 Jan 2024 11:00:00 +0000
Message-ID: <msg2@example.com>
In-Reply-To: <msg1@example.com>

Body
"#,
        );

        let loader = FixtureLoader::with_base_path(temp_dir.path().to_path_buf());
        let fixtures = loader.load_directory("thread-01").unwrap();

        assert_eq!(fixtures.len(), 2);
        // Should be sorted by filename
        assert!(fixtures[0].source_path.contains("01-initial"));
        assert!(fixtures[1].source_path.contains("02-reply"));
    }

    #[test]
    fn test_extract_email() {
        assert_eq!(extract_email("user@example.com"), "user@example.com");
        assert_eq!(extract_email("John Doe <john@example.com>"), "john@example.com");
        assert_eq!(extract_email("<bare@example.com>"), "bare@example.com");
        assert_eq!(extract_email("  spaced@example.com  "), "spaced@example.com");
    }
}

