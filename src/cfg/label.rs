// src/cfg/label.rs

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq)]
pub enum Label {
    Inbox,
    Important, // Gmail “Important”
    Starred,   // Gmail “Starred” (aka IMAP \Flagged)
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
            "INBOX" => Label::Inbox,
            "IMPORTANT" => Label::Important,
            "FLAGGED" | "STARRED" => Label::Starred,
            "SENT" => Label::Sent,
            "DRAFT" => Label::Draft,
            "TRASH" => Label::Trash,
            "SPAM" => Label::Spam,
            _other => Label::Custom(trimmed.to_string()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_label_inbox() {
        assert_eq!(Label::new("INBOX"), Label::Inbox);
        assert_eq!(Label::new("inbox"), Label::Inbox);
        assert_eq!(Label::new("Inbox"), Label::Inbox);
    }

    #[test]
    fn test_label_important() {
        assert_eq!(Label::new("IMPORTANT"), Label::Important);
        assert_eq!(Label::new("important"), Label::Important);
        assert_eq!(Label::new("\\Important"), Label::Important);
    }

    #[test]
    fn test_label_starred() {
        assert_eq!(Label::new("STARRED"), Label::Starred);
        assert_eq!(Label::new("starred"), Label::Starred);
        assert_eq!(Label::new("FLAGGED"), Label::Starred);
        assert_eq!(Label::new("\\Flagged"), Label::Starred);
    }

    #[test]
    fn test_label_sent() {
        assert_eq!(Label::new("SENT"), Label::Sent);
        assert_eq!(Label::new("sent"), Label::Sent);
    }

    #[test]
    fn test_label_draft() {
        assert_eq!(Label::new("DRAFT"), Label::Draft);
        assert_eq!(Label::new("\\Draft"), Label::Draft);
    }

    #[test]
    fn test_label_trash() {
        assert_eq!(Label::new("TRASH"), Label::Trash);
        assert_eq!(Label::new("trash"), Label::Trash);
    }

    #[test]
    fn test_label_spam() {
        assert_eq!(Label::new("SPAM"), Label::Spam);
        assert_eq!(Label::new("spam"), Label::Spam);
    }

    #[test]
    fn test_label_custom() {
        assert_eq!(Label::new("MyLabel"), Label::Custom("MyLabel".to_string()));
        assert_eq!(Label::new("work/projects"), Label::Custom("work/projects".to_string()));
    }

    #[test]
    fn test_label_strips_backslash() {
        // \Seen should become Custom("Seen") since Seen isn't a known label
        assert_eq!(Label::new("\\Seen"), Label::Custom("Seen".to_string()));
    }

    #[test]
    fn test_label_deserialize() {
        let yaml = "\"INBOX\"";
        let label: Label = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(label, Label::Inbox);

        let yaml2 = "\"CustomLabel\"";
        let label2: Label = serde_yaml::from_str(yaml2).unwrap();
        assert_eq!(label2, Label::Custom("CustomLabel".to_string()));
    }
}
