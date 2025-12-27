// src/cfg/message_filter.rs

use crate::cfg::label::Label;
use crate::message::{EmailAddress, Message};
use globset::Glob;
use serde::de::{self, Deserializer};
use serde::Deserialize;
use serde_yaml::{from_value, Value};
use std::collections::HashMap;

#[derive(Debug, PartialEq, Clone, Deserialize)]
pub struct AddressFilter {
    pub patterns: Vec<String>,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
pub enum FilterAction {
    Star,
    Flag,
    Move(String),
}

/// Helper to deserialize the `labels:` section of your YAML.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct LabelsFilter {
    pub included: Vec<Label>,
    pub excluded: Vec<Label>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageFilter {
    #[serde(skip_deserializing)]
    pub name: String,

    #[serde(default)]
    #[serde(deserialize_with = "deserialize_opt_address_filter")]
    pub to: Option<AddressFilter>,

    #[serde(default)]
    #[serde(deserialize_with = "deserialize_opt_address_filter")]
    pub cc: Option<AddressFilter>,

    #[serde(default)]
    #[serde(deserialize_with = "deserialize_opt_address_filter")]
    pub from: Option<AddressFilter>,

    #[serde(default)]
    pub subject: Vec<String>,

    #[serde(default)]
    #[serde(alias = "label")]
    #[serde(deserialize_with = "deserialize_labels_filter")]
    pub labels: LabelsFilter,

    /// Custom header matching: header name -> glob patterns
    /// Example: { "List-Id": ["*github*"], "X-Priority": ["1"] }
    #[serde(default)]
    pub headers: HashMap<String, Vec<String>>,

    #[serde(default)]
    #[serde(alias = "action")]
    #[serde(deserialize_with = "deserialize_actions")]
    pub actions: Vec<FilterAction>,
}

impl AddressFilter {
    /// Returns true if **any** of the `emails` matches **any** glob in `self.patterns`.
    pub fn matches(&self, emails: &[String]) -> bool {
        for pat in &self.patterns {
            let matcher = Glob::new(pat).expect("invalid glob").compile_matcher();
            for email in emails {
                if matcher.is_match(email) {
                    return true;
                }
            }
        }
        false
    }
}

impl MessageFilter {
    /// Returns true if this filter matches the given message.
    pub fn matches(&self, msg: &Message) -> bool {
        // helper to extract just the email‑strings
        let extract = |addrs: &Vec<EmailAddress>| addrs.iter().map(|ea| ea.email.clone()).collect::<Vec<_>>();

        // TO
        if let Some(ref af) = self.to {
            let emails = extract(&msg.to);
            if af.patterns.is_empty() {
                if !emails.is_empty() {
                    return false;
                }
            } else if !af.matches(&emails) {
                return false;
            }
        }
        // CC
        if let Some(ref af) = self.cc {
            let emails = extract(&msg.cc);
            if af.patterns.is_empty() {
                if !emails.is_empty() {
                    return false;
                }
            } else if !af.matches(&emails) {
                return false;
            }
        }
        // FROM
        if let Some(ref af) = self.from {
            let emails = extract(&msg.from);
            if af.patterns.is_empty() {
                if !emails.is_empty() {
                    return false;
                }
            } else if !af.matches(&emails) {
                return false;
            }
        }

        // SUBJECT globs
        if !self.subject.is_empty() {
            let mut found = false;
            for pat in &self.subject {
                let matcher = Glob::new(pat).unwrap().compile_matcher();
                if matcher.is_match(&msg.subject) {
                    found = true;
                    break;
                }
            }
            if !found {
                return false;
            }
        }

        // LABELS: included must _appear_; excluded must _not_
        if !self.labels.included.is_empty() && !msg.labels.iter().any(|l| self.labels.included.contains(l)) {
            return false;
        }
        if !self.labels.excluded.is_empty() && msg.labels.iter().any(|l| self.labels.excluded.contains(l)) {
            return false;
        }

        // HEADERS: custom header matching
        for (header_name, patterns) in &self.headers {
            if let Some(header_value) = msg.headers.get(header_name) {
                // At least one pattern must match the header value
                let mut matched = false;
                for pat in patterns {
                    let matcher = Glob::new(pat).expect("invalid glob").compile_matcher();
                    if matcher.is_match(header_value) {
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    return false;
                }
            } else {
                // Header not present, patterns don't match
                return false;
            }
        }

        true
    }
}

/// Custom deserializer for `to`, `cc`, `from`:
fn deserialize_opt_address_filter<'de, D>(deserializer: D) -> Result<Option<AddressFilter>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(deserializer).map_err(de::Error::custom)?;
    match v {
        Value::Null => Ok(None),
        Value::Sequence(seq) => {
            let mut patterns = Vec::new();
            for val in seq {
                if let Value::String(s) = val {
                    patterns.push(s);
                } else {
                    return Err(de::Error::custom("Invalid entry in address filter"));
                }
            }
            Ok(Some(AddressFilter { patterns }))
        }
        Value::String(s) => Ok(Some(AddressFilter { patterns: vec![s] })),
        other @ Value::Mapping(_) => {
            // map mapping → AddressFilter via YAML
            let af: AddressFilter = from_value(other).map_err(de::Error::custom)?;
            Ok(Some(af))
        }
        _ => Err(de::Error::custom("Invalid address filter format")),
    }
}

fn deserialize_labels_filter<'de, D>(deserializer: D) -> Result<LabelsFilter, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(deserializer).map_err(de::Error::custom)?;
    match v {
        Value::String(s) => Ok(LabelsFilter {
            included: vec![Label::new(&s)],
            excluded: vec![],
        }),
        Value::Sequence(seq) => {
            let mut included = Vec::new();
            for val in seq {
                match val {
                    Value::String(s) => included.push(Label::new(&s)),
                    _ => return Err(de::Error::custom("Invalid label entry")),
                }
            }
            Ok(LabelsFilter {
                included,
                excluded: vec![],
            })
        }
        Value::Mapping(map) => {
            let mut included = Vec::new();
            let mut excluded = Vec::new();
            for (k, v) in map {
                let key = match k {
                    Value::String(s) => s,
                    _ => return Err(de::Error::custom("Non-string key in labels map")),
                };
                match key.as_str() {
                    "included" => {
                        if let Value::Sequence(seq) = v {
                            for inner in seq {
                                if let Value::String(s) = inner {
                                    included.push(Label::new(&s));
                                } else {
                                    return Err(de::Error::custom("Invalid included label"));
                                }
                            }
                        } else {
                            return Err(de::Error::custom("`included` must be a sequence"));
                        }
                    }
                    "excluded" => {
                        if let Value::Sequence(seq) = v {
                            for inner in seq {
                                if let Value::String(s) = inner {
                                    excluded.push(Label::new(&s));
                                } else {
                                    return Err(de::Error::custom("Invalid excluded label"));
                                }
                            }
                        } else {
                            return Err(de::Error::custom("`excluded` must be a sequence"));
                        }
                    }
                    other => return Err(de::Error::unknown_field(other, &["included", "excluded"])),
                }
            }
            Ok(LabelsFilter { included, excluded })
        }
        _ => Err(de::Error::custom("Invalid `labels` value")),
    }
}

fn deserialize_actions<'de, D>(deserializer: D) -> Result<Vec<FilterAction>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(deserializer).map_err(de::Error::custom)?;
    let mut out = Vec::new();
    match v {
        Value::String(s) => {
            let act = match s.as_str() {
                "Star" => FilterAction::Star,
                "Flag" => FilterAction::Flag,
                other => FilterAction::Move(other.to_string()),
            };
            out.push(act);
        }
        Value::Sequence(seq) => {
            for val in seq {
                if let Value::String(s) = val {
                    let act = match s.as_str() {
                        "Star" => FilterAction::Star,
                        "Flag" => FilterAction::Flag,
                        other => FilterAction::Move(other.to_string()),
                    };
                    out.push(act);
                } else {
                    return Err(de::Error::custom("Invalid entry in actions list"));
                }
            }
        }
        _ => return Err(de::Error::custom("Invalid `action` value")),
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_message(to: Vec<&str>, cc: Vec<&str>, from: &str, subject: &str) -> Message {
        let to_header = if to.is_empty() { String::new() } else { format!("To: {}\r\n", to.join(", ")) };
        let cc_header = if cc.is_empty() { String::new() } else { format!("Cc: {}\r\n", cc.join(", ")) };

        let headers = format!(
            "{}{}From: {}\r\nSubject: {}\r\n\r\n",
            to_header, cc_header, from, subject
        );

        Message::new(
            1,
            1,
            headers.into_bytes(),
            vec![],
            "2024-01-01T00:00:00+00:00".to_string(),
            None,
        )
    }

    #[test]
    fn test_address_filter_matches_exact() {
        let filter = AddressFilter {
            patterns: vec!["test@example.com".to_string()],
        };
        assert!(filter.matches(&["test@example.com".to_string()]));
        assert!(!filter.matches(&["other@example.com".to_string()]));
    }

    #[test]
    fn test_address_filter_matches_glob() {
        let filter = AddressFilter {
            patterns: vec!["*@example.com".to_string()],
        };
        assert!(filter.matches(&["test@example.com".to_string()]));
        assert!(filter.matches(&["anyone@example.com".to_string()]));
        assert!(!filter.matches(&["test@other.com".to_string()]));
    }

    #[test]
    fn test_address_filter_multiple_patterns() {
        let filter = AddressFilter {
            patterns: vec!["*@example.com".to_string(), "*@test.com".to_string()],
        };
        assert!(filter.matches(&["user@example.com".to_string()]));
        assert!(filter.matches(&["user@test.com".to_string()]));
        assert!(!filter.matches(&["user@other.com".to_string()]));
    }

    #[test]
    fn test_message_filter_matches_to() {
        let filter = MessageFilter {
            name: "test".to_string(),
            to: Some(AddressFilter {
                patterns: vec!["me@example.com".to_string()],
            }),
            cc: None,
            from: None,
            subject: vec![],
            labels: LabelsFilter::default(),
            headers: HashMap::new(),
            actions: vec![FilterAction::Star],
        };

        let msg = make_test_message(vec!["me@example.com"], vec![], "sender@example.com", "Test");
        assert!(filter.matches(&msg));

        let msg2 = make_test_message(vec!["other@example.com"], vec![], "sender@example.com", "Test");
        assert!(!filter.matches(&msg2));
    }

    #[test]
    fn test_message_filter_requires_empty_cc() {
        let filter = MessageFilter {
            name: "test".to_string(),
            to: None,
            cc: Some(AddressFilter { patterns: vec![] }), // empty = require no CC
            from: None,
            subject: vec![],
            labels: LabelsFilter::default(),
            headers: HashMap::new(),
            actions: vec![FilterAction::Star],
        };

        // Message with no CC should match
        let msg_no_cc = make_test_message(vec!["to@example.com"], vec![], "from@example.com", "Test");
        assert!(filter.matches(&msg_no_cc));

        // Message with CC should NOT match
        let msg_with_cc = make_test_message(
            vec!["to@example.com"],
            vec!["cc@example.com"],
            "from@example.com",
            "Test",
        );
        assert!(!filter.matches(&msg_with_cc));
    }

    #[test]
    fn test_message_filter_matches_from() {
        let filter = MessageFilter {
            name: "test".to_string(),
            to: None,
            cc: None,
            from: Some(AddressFilter {
                patterns: vec!["*@company.com".to_string()],
            }),
            subject: vec![],
            labels: LabelsFilter::default(),
            headers: HashMap::new(),
            actions: vec![FilterAction::Star],
        };

        let msg = make_test_message(vec!["me@example.com"], vec![], "boss@company.com", "Important");
        assert!(filter.matches(&msg));

        let msg2 = make_test_message(vec!["me@example.com"], vec![], "spam@other.com", "Spam");
        assert!(!filter.matches(&msg2));
    }

    #[test]
    fn test_message_filter_matches_subject_glob() {
        let filter = MessageFilter {
            name: "test".to_string(),
            to: None,
            cc: None,
            from: None,
            subject: vec!["*urgent*".to_string()],
            labels: LabelsFilter::default(),
            headers: HashMap::new(),
            actions: vec![FilterAction::Star],
        };

        let msg = make_test_message(
            vec!["me@example.com"],
            vec![],
            "from@example.com",
            "This is urgent please read",
        );
        assert!(filter.matches(&msg));

        let msg2 = make_test_message(vec!["me@example.com"], vec![], "from@example.com", "Normal message");
        assert!(!filter.matches(&msg2));
    }

    #[test]
    fn test_message_filter_combined_conditions() {
        // Filter: emails to me, from @company.com, with no CC
        let filter = MessageFilter {
            name: "only-me-from-company".to_string(),
            to: Some(AddressFilter {
                patterns: vec!["me@example.com".to_string()],
            }),
            cc: Some(AddressFilter { patterns: vec![] }), // no CC
            from: Some(AddressFilter {
                patterns: vec!["*@company.com".to_string()],
            }),
            subject: vec![],
            labels: LabelsFilter::default(),
            headers: HashMap::new(),
            actions: vec![FilterAction::Star],
        };

        // Should match: to me, from company, no CC
        let good = make_test_message(vec!["me@example.com"], vec![], "boss@company.com", "Good");
        assert!(filter.matches(&good));

        // Should NOT match: has CC
        let with_cc = make_test_message(
            vec!["me@example.com"],
            vec!["other@example.com"],
            "boss@company.com",
            "CC",
        );
        assert!(!filter.matches(&with_cc));

        // Should NOT match: wrong sender
        let wrong_from = make_test_message(vec!["me@example.com"], vec![], "spam@other.com", "Spam");
        assert!(!filter.matches(&wrong_from));
    }

    #[test]
    fn test_message_filter_matches_custom_header() {
        // Create a filter that requires List-Id header with github pattern
        let mut header_patterns = HashMap::new();
        header_patterns.insert("List-Id".to_string(), vec!["*github*".to_string()]);

        let filter = MessageFilter {
            name: "github-lists".to_string(),
            to: None,
            cc: None,
            from: None,
            subject: vec![],
            labels: LabelsFilter::default(),
            headers: header_patterns,
            actions: vec![FilterAction::Move("GitHub".to_string())],
        };

        // Create a message with List-Id header
        let headers = b"From: noreply@github.com\r\n\
                        To: user@example.com\r\n\
                        Subject: [repo] Issue opened\r\n\
                        List-Id: <repo.github.com>\r\n\
                        \r\n"
            .to_vec();
        let msg = Message::new(1, 1, headers, vec![], "2024-01-01T00:00:00+00:00".to_string(), None);
        assert!(filter.matches(&msg));

        // Message without List-Id should NOT match
        let no_list_id = make_test_message(vec!["user@example.com"], vec![], "noreply@github.com", "Issue");
        assert!(!filter.matches(&no_list_id));
    }

    #[test]
    fn test_message_filter_header_must_match_pattern() {
        let mut header_patterns = HashMap::new();
        header_patterns.insert("X-Priority".to_string(), vec!["1".to_string()]);

        let filter = MessageFilter {
            name: "high-priority".to_string(),
            to: None,
            cc: None,
            from: None,
            subject: vec![],
            labels: LabelsFilter::default(),
            headers: header_patterns,
            actions: vec![FilterAction::Flag],
        };

        // High priority message
        let high_priority = b"From: boss@company.com\r\n\
                              To: me@example.com\r\n\
                              Subject: Urgent\r\n\
                              X-Priority: 1\r\n\
                              \r\n"
            .to_vec();
        let msg = Message::new(
            1,
            1,
            high_priority,
            vec![],
            "2024-01-01T00:00:00+00:00".to_string(),
            None,
        );
        assert!(filter.matches(&msg));

        // Low priority message should NOT match
        let low_priority = b"From: newsletter@spam.com\r\n\
                             To: me@example.com\r\n\
                             Subject: Newsletter\r\n\
                             X-Priority: 5\r\n\
                             \r\n"
            .to_vec();
        let msg2 = Message::new(
            2,
            2,
            low_priority,
            vec![],
            "2024-01-01T00:00:00+00:00".to_string(),
            None,
        );
        assert!(!filter.matches(&msg2));
    }
}
