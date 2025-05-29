// src/cfg/label.rs

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq)]
pub enum Label {
    Inbox,
    Important,    // IMAP \Flagged → “Important”
    Starred,      // Gmail STARRED
    Sent,
    Draft,
    Trash,
    Spam,
    Custom(String),
}

impl Label {
    /// Construct from the raw string returned by X-GM-LABELS.
    /// E.g. "\\Inbox" → Label::Inbox, "MyProject" → Label::Custom("MyProject")
    pub fn new(raw: &str) -> Self {
        match raw {
            "\\Inbox"       | "INBOX"    => Label::Inbox,
            "\\Important"   | "IMPORTANT"=> Label::Important,
            "\\Flagged"     | "STARRED"  => Label::Starred,
            "\\Sent"        | "SENT"     => Label::Sent,
            "\\Draft"       | "DRAFT"    => Label::Draft,
            "\\Trash"       | "TRASH"    => Label::Trash,
            "\\Spam"        | "SPAM"     => Label::Spam,
            other                         => Label::Custom(other.to_string()),
        }
    }
}

// manually deserialize any YAML string into our Label::new
impl<'de> Deserialize<'de> for Label {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Ok(Label::new(&raw))
    }
}
