// src/cfg/label.rs

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq)]
pub enum Label {
    Inbox,
    Important,    // Gmail “Important”
    Starred,      // Gmail “Starred” (aka IMAP \Flagged)
    Sent,
    Draft,
    Trash,
    Spam,
    Custom(String),
}

impl Label {
    /// Construct from the raw string returned by X-GM-LABELS or your YAML.
    pub fn new(raw: &str) -> Self {
        // strip any leading backslashes, then uppercase for matching
        let trimmed = raw.trim_start_matches('\\');
        let up = trimmed.to_uppercase();
        match up.as_str() {
            "INBOX"      => Label::Inbox,
            "IMPORTANT"  => Label::Important,
            "FLAGGED" |
            "STARRED"    => Label::Starred,
            "SENT"       => Label::Sent,
            "DRAFT"      => Label::Draft,
            "TRASH"      => Label::Trash,
            "SPAM"       => Label::Spam,
            _other       => Label::Custom(trimmed.to_string()),
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
