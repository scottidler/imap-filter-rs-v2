// src/message.rs

use mailparse::{addrparse, MailAddr};
use std::collections::HashMap;

use crate::cfg::label::Label;

#[derive(Debug, Clone)]
pub struct EmailAddress {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub uid: u32,
    pub seq: u32,
    pub to: Vec<EmailAddress>,
    pub cc: Vec<EmailAddress>,
    pub from: Vec<EmailAddress>,
    pub subject: String,
    pub date: String,
    pub labels: Vec<Label>,
    pub headers: HashMap<String, String>,
    // Thread-related fields for standard IMAP thread grouping
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub thread_id: Option<String>, // Gmail X-GM-THRID
}

impl Message {
    /// Create a new Message from raw IMAP data.
    pub fn new(
        uid: u32,
        seq: u32,
        raw_headers: Vec<u8>,
        raw_labels: Vec<String>,
        internal_date: String,
        gmail_thread_id: Option<String>,
    ) -> Self {
        // parse headers
        let raw_str = String::from_utf8_lossy(&raw_headers);
        let headers: HashMap<_, _> = raw_str
            .lines()
            .filter_map(|line| line.split_once(": "))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        // owned parsing of address fields
        let mut to = parse_addrs(headers.get("To"));
        let cc = parse_addrs(headers.get("Cc"));
        let from = parse_addrs(headers.get("From"));

        // if no "To:", but we do have "Delivered-To:", treat that as the recipient
        if to.is_empty() {
            to = parse_addrs(headers.get("Delivered-To"));
        }

        // labels and subject
        let labels = raw_labels.into_iter().map(|s| Label::new(&s)).collect();
        let subject = headers.get("Subject").cloned().unwrap_or_default();

        // Parse thread-related headers (for non-Gmail IMAP servers - Phase 2)
        let message_id = headers.get("Message-ID").cloned();
        let in_reply_to = headers.get("In-Reply-To").cloned();
        let references = headers
            .get("References")
            .map(|refs| refs.split_whitespace().map(String::from).collect())
            .unwrap_or_default();

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
            thread_id: gmail_thread_id,
        }
    }

    /// Get the display name of the first sender, or their email if no name
    pub fn sender_display(&self) -> String {
        self.from
            .first()
            .map(
                |addr| {
                    if addr.name.is_empty() {
                        addr.email.clone()
                    } else {
                        addr.name.clone()
                    }
                },
            )
            .unwrap_or_default()
    }
}

/// Owned parsing of an address header into `EmailAddress`
fn parse_addrs(field: Option<&String>) -> Vec<EmailAddress> {
    if let Some(s) = field {
        if let Ok(addrs) = addrparse(s) {
            let mut result = Vec::new();
            for addr in addrs.iter() {
                match addr {
                    MailAddr::Single(info) => {
                        result.push(EmailAddress {
                            name: info.display_name.clone().unwrap_or_default(),
                            email: info.addr.clone(),
                        });
                    }
                    MailAddr::Group(group) => {
                        for info in &group.addrs {
                            result.push(EmailAddress {
                                name: info.display_name.clone().unwrap_or_default(),
                                email: info.addr.clone(),
                            });
                        }
                    }
                }
            }
            return result;
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_headers() -> Vec<u8> {
        b"From: Test User <test@example.com>\r\n\
          To: recipient@example.com\r\n\
          Cc: cc@example.com\r\n\
          Subject: Test Subject\r\n\
          Message-ID: <123@example.com>\r\n\
          In-Reply-To: <parent@example.com>\r\n\
          References: <root@example.com> <parent@example.com>\r\n\
          \r\n"
            .to_vec()
    }

    #[test]
    fn test_message_new_parses_headers() {
        let headers = make_test_headers();
        let labels = vec!["INBOX".to_string(), "Important".to_string()];
        let msg = Message::new(
            12345,
            1,
            headers,
            labels,
            "2024-01-15T10:00:00+00:00".to_string(),
            Some("thread123".to_string()),
        );

        assert_eq!(msg.uid, 12345);
        assert_eq!(msg.seq, 1);
        assert_eq!(msg.subject, "Test Subject");
        assert_eq!(msg.thread_id, Some("thread123".to_string()));
        assert_eq!(msg.from.len(), 1);
        assert_eq!(msg.from[0].email, "test@example.com");
        assert_eq!(msg.from[0].name, "Test User");
        assert_eq!(msg.to.len(), 1);
        assert_eq!(msg.to[0].email, "recipient@example.com");
        assert_eq!(msg.cc.len(), 1);
        assert_eq!(msg.cc[0].email, "cc@example.com");
    }

    #[test]
    fn test_message_thread_headers() {
        let headers = make_test_headers();
        let msg = Message::new(1, 1, headers, vec![], "2024-01-15T10:00:00+00:00".to_string(), None);

        assert_eq!(msg.message_id, Some("<123@example.com>".to_string()));
        assert_eq!(msg.in_reply_to, Some("<parent@example.com>".to_string()));
        assert_eq!(msg.references.len(), 2);
        assert!(msg.references.contains(&"<root@example.com>".to_string()));
        assert!(msg.references.contains(&"<parent@example.com>".to_string()));
    }

    #[test]
    fn test_message_uses_delivered_to_when_no_to() {
        let headers = b"From: sender@example.com\r\n\
                        Delivered-To: delivered@example.com\r\n\
                        Subject: No To Header\r\n\
                        \r\n"
            .to_vec();

        let msg = Message::new(1, 1, headers, vec![], "2024-01-15T10:00:00+00:00".to_string(), None);

        assert_eq!(msg.to.len(), 1);
        assert_eq!(msg.to[0].email, "delivered@example.com");
    }

    #[test]
    fn test_message_labels_converted() {
        let labels = vec!["INBOX".to_string(), "Starred".to_string(), "CustomLabel".to_string()];
        let msg = Message::new(
            1,
            1,
            b"From: test@example.com\r\n\r\n".to_vec(),
            labels,
            "2024-01-15T10:00:00+00:00".to_string(),
            None,
        );

        assert_eq!(msg.labels.len(), 3);
        assert!(msg.labels.iter().any(|l| matches!(l, Label::Inbox)));
        assert!(msg.labels.iter().any(|l| matches!(l, Label::Starred)));
    }

    #[test]
    fn test_message_headers_stored() {
        let headers = make_test_headers();
        let msg = Message::new(1, 1, headers, vec![], "2024-01-15T10:00:00+00:00".to_string(), None);

        assert!(msg.headers.contains_key("From"));
        assert!(msg.headers.contains_key("Subject"));
    }

    #[test]
    fn test_sender_display_with_name() {
        let msg = Message::new(
            1,
            1,
            b"From: John Doe <john@example.com>\r\n\r\n".to_vec(),
            vec![],
            "2024-01-15T10:00:00+00:00".to_string(),
            None,
        );
        assert_eq!(msg.sender_display(), "John Doe");
    }

    #[test]
    fn test_sender_display_without_name() {
        let msg = Message::new(
            1,
            1,
            b"From: john@example.com\r\n\r\n".to_vec(),
            vec![],
            "2024-01-15T10:00:00+00:00".to_string(),
            None,
        );
        assert_eq!(msg.sender_display(), "john@example.com");
    }
}
