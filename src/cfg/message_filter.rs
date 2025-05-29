// src/message_filter.rs

use serde::Deserialize;
use serde::de::{self, Deserializer};
use serde_yaml::Value;
use globset::Glob;

use crate::cfg::label::Label;
use crate::message::{EmailAddress, Message};

#[derive(Debug, PartialEq, Clone, Deserialize)]
pub struct AddressFilter {
    pub patterns: Vec<String>,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
pub struct SubjectFilter {
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
    pub to: Option<AddressFilter>,

    #[serde(default)]
    pub cc: Option<AddressFilter>,

    #[serde(default)]
    pub from: Option<AddressFilter>,

    #[serde(default)]
    pub subject: Vec<String>,

    #[serde(default)]
    #[serde(alias = "label")]
    #[serde(deserialize_with = "deserialize_labels_filter")]
    pub labels: LabelsFilter,

    #[serde(default)]
    #[serde(alias = "action")]
    #[serde(deserialize_with = "deserialize_actions")]
    pub actions: Vec<FilterAction>,
}

impl AddressFilter {
    /// Returns true if **any** of the `emails` matches **any** glob in `self.patterns`.
    pub fn matches(&self, emails: &[String]) -> bool {
        for pat in &self.patterns {
            let matcher = Glob::new(pat)
                .expect("invalid glob")
                .compile_matcher();
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
        // helper to extract just the email‐strings
        let extract = |addrs: &Vec<EmailAddress>| {
            addrs.iter().map(|ea| ea.email.clone()).collect::<Vec<_>>()
        };

        // TO
        if let Some(ref af) = self.to {
            let emails = extract(&msg.to);
            if af.patterns.is_empty() {
                if !emails.is_empty() { return false; }
            } else if !af.matches(&emails) {
                return false;
            }
        }
        // CC
        if let Some(ref af) = self.cc {
            let emails = extract(&msg.cc);
            if af.patterns.is_empty() {
                if !emails.is_empty() { return false; }
            } else if !af.matches(&emails) {
                return false;
            }
        }
        // FROM
        if let Some(ref af) = self.from {
            let emails = extract(&msg.from);
            if af.patterns.is_empty() {
                if !emails.is_empty() { return false; }
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
        if !self.labels.included.is_empty() {
            if !msg.labels.iter().any(|l| self.labels.included.contains(l)) {
                return false;
            }
        }
        if !self.labels.excluded.is_empty() {
            if msg.labels.iter().any(|l| self.labels.excluded.contains(l)) {
                return false;
            }
        }

        true
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
            Ok(LabelsFilter { included, excluded: vec![] })
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
                    other => {
                        return Err(de::Error::unknown_field(&other, &["included", "excluded"]))
                    }
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
