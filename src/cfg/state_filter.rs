// src/cfg/state_filter.rs

use chrono;
use chrono::{DateTime, Utc};
use eyre::eyre;
use serde::de::{self, Deserializer};
use serde::Deserialize;
use serde_yaml::Value;

use crate::cfg::label::Label;
use crate::message::Message;
use crate::utils::parse_days;

#[derive(Clone, Debug, PartialEq)]
pub enum Ttl {
    Keep,
    Days(chrono::Duration),
    Detailed {
        read: chrono::Duration,
        unread: chrono::Duration,
    },
}

impl<'de> Deserialize<'de> for Ttl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct TtlVisitor;

        impl<'de> de::Visitor<'de> for TtlVisitor {
            type Value = Ttl;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("Keep, '<n>d', or { read: '<n>d', unread: '<n>d' }")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if value == "Keep" {
                    Ok(Ttl::Keep)
                } else {
                    parse_days(value)
                        .map(Ttl::Days)
                        .map_err(|e| E::custom(format!("Invalid TTL '{}': {}", value, e)))
                }
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let mut read = None;
                let mut unread = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "read" => {
                            let v: String = map.next_value()?;
                            read = Some(parse_days(&v).map_err(|e| de::Error::custom(e.to_string()))?);
                        }
                        "unread" => {
                            let v: String = map.next_value()?;
                            unread = Some(parse_days(&v).map_err(|e| de::Error::custom(e.to_string()))?);
                        }
                        other => return Err(de::Error::unknown_field(other, &["read", "unread"])),
                    }
                }

                let read = read.ok_or_else(|| de::Error::missing_field("read"))?;
                let unread = unread.ok_or_else(|| de::Error::missing_field("unread"))?;
                Ok(Ttl::Detailed { read, unread })
            }
        }

        deserializer.deserialize_any(TtlVisitor)
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub enum StateAction {
    Move(String),
    Delete,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct StateFilter {
    /// Map‐key → this field is set in `deserialize_named_states`
    #[serde(skip_deserializing, default)]
    pub name: String,

    /// support `label: Foo` or `labels: [...]`
    #[serde(default)]
    #[serde(alias = "label")]
    #[serde(deserialize_with = "deserialize_labels_vec")]
    pub labels: Vec<Label>,

    /// **required** in YAML
    pub ttl: Ttl,

    /// support bare string or `{ Move: X }`
    #[serde(default = "default_action")]
    #[serde(alias = "action")]
    #[serde(deserialize_with = "deserialize_state_action")]
    pub action: StateAction,

    /// optional, defaults to false
    #[serde(default)]
    pub nerf: bool,
}

impl StateFilter {
    /// Only messages carrying _any_ of these labels (or all if empty) participate.
    pub fn matches(&self, msg: &Message) -> bool {
        if self.labels.is_empty() {
            return true;
        }
        msg.labels.iter().any(|l| self.labels.contains(l))
    }

    /// Returns:
    ///  - `Ok(None)` if TTL == Keep or not yet expired
    ///  - `Ok(Some(action))` if TTL expired and we should apply `action`
    pub fn evaluate_ttl(&self, msg: &Message, now: DateTime<Utc>) -> eyre::Result<Option<StateAction>> {
        // parse the stored RFC3339 date back into a chrono DateTime
        let internal: DateTime<Utc> = DateTime::parse_from_rfc3339(&msg.date)
            .map_err(|e| eyre!("Bad INTERNALDATE '{}': {}", msg.date, e))?
            .with_timezone(&Utc);

        let age = now.signed_duration_since(internal);

        // Check if message is read (has \Seen flag)
        let is_read = msg
            .labels
            .iter()
            .any(|l| matches!(l, Label::Custom(s) if s == "Seen" || s == "\\Seen"));

        let ttl_duration = match &self.ttl {
            Ttl::Keep => return Ok(None),
            Ttl::Days(dur) => *dur,
            Ttl::Detailed { read, unread } => {
                if is_read {
                    *read
                } else {
                    *unread
                }
            }
        };

        if age >= ttl_duration {
            Ok(Some(self.action.clone()))
        } else {
            Ok(None)
        }
    }
}

fn deserialize_labels_vec<'de, D>(deserializer: D) -> Result<Vec<Label>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(deserializer).map_err(de::Error::custom)?;
    match v {
        Value::String(s) => Ok(vec![Label::new(&s)]),
        Value::Sequence(seq) => seq
            .into_iter()
            .map(|val| {
                if let Value::String(s) = val {
                    Ok(Label::new(&s))
                } else {
                    Err(de::Error::custom("Invalid label entry"))
                }
            })
            .collect(),
        _ => Err(de::Error::custom("Invalid `labels` value")),
    }
}

// src/cfg/state_filter.rs

fn deserialize_state_action<'de, D>(deserializer: D) -> Result<StateAction, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(deserializer).map_err(de::Error::custom)?;
    match v {
        Value::String(s) => Ok(StateAction::Move(s)),
        Value::Mapping(m) => {
            if m.len() != 1 {
                return Err(de::Error::custom("Expected single key in action map"));
            }
            let (k, v) = m.into_iter().next().unwrap();
            let key = if let Value::String(s) = k {
                s
            } else {
                return Err(de::Error::custom("Invalid action key"));
            };
            let target = if let Value::String(s) = v {
                s
            } else {
                return Err(de::Error::custom("Invalid action target"));
            };
            match key.as_str() {
                "Move" => Ok(StateAction::Move(target)),
                "Delete" => Ok(StateAction::Delete),
                other => Err(de::Error::unknown_field(other, &["Move", "Delete"])),
            }
        }
        _ => Err(de::Error::custom("Invalid `action` value")),
    }
}

fn default_action() -> StateAction {
    StateAction::Move(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn make_test_message(date: &str, labels: Vec<&str>) -> Message {
        Message::new(
            1,
            1,
            b"From: test@example.com\r\nTo: recipient@example.com\r\n\r\n".to_vec(),
            labels.into_iter().map(String::from).collect(),
            date.to_string(),
            None,
        )
    }

    #[test]
    fn test_ttl_keep_never_expires() {
        let filter = StateFilter {
            name: "test".to_string(),
            labels: vec![],
            ttl: Ttl::Keep,
            action: StateAction::Move("Archive".to_string()),
            nerf: false,
        };

        let msg = make_test_message("2020-01-01T00:00:00+00:00", vec![]);
        let now = Utc::now();

        // Even with a very old message, Keep should never expire
        assert!(filter.evaluate_ttl(&msg, now).unwrap().is_none());
    }

    #[test]
    fn test_ttl_days_expired() {
        let filter = StateFilter {
            name: "test".to_string(),
            labels: vec![],
            ttl: Ttl::Days(Duration::days(7)),
            action: StateAction::Move("Archive".to_string()),
            nerf: false,
        };

        // Message from 10 days ago
        let ten_days_ago = Utc::now() - Duration::days(10);
        let msg = make_test_message(&ten_days_ago.to_rfc3339(), vec![]);
        let now = Utc::now();

        let result = filter.evaluate_ttl(&msg, now).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), StateAction::Move("Archive".to_string()));
    }

    #[test]
    fn test_ttl_days_not_expired() {
        let filter = StateFilter {
            name: "test".to_string(),
            labels: vec![],
            ttl: Ttl::Days(Duration::days(7)),
            action: StateAction::Move("Archive".to_string()),
            nerf: false,
        };

        // Message from 3 days ago
        let three_days_ago = Utc::now() - Duration::days(3);
        let msg = make_test_message(&three_days_ago.to_rfc3339(), vec![]);
        let now = Utc::now();

        assert!(filter.evaluate_ttl(&msg, now).unwrap().is_none());
    }

    #[test]
    fn test_ttl_detailed_read_message() {
        let filter = StateFilter {
            name: "test".to_string(),
            labels: vec![],
            ttl: Ttl::Detailed {
                read: Duration::days(7),
                unread: Duration::days(21),
            },
            action: StateAction::Move("Archive".to_string()),
            nerf: false,
        };

        // Read message from 10 days ago (past read TTL of 7 days)
        let ten_days_ago = Utc::now() - Duration::days(10);
        let msg = make_test_message(&ten_days_ago.to_rfc3339(), vec!["Seen"]);
        let now = Utc::now();

        let result = filter.evaluate_ttl(&msg, now).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn test_ttl_detailed_unread_message_not_expired() {
        let filter = StateFilter {
            name: "test".to_string(),
            labels: vec![],
            ttl: Ttl::Detailed {
                read: Duration::days(7),
                unread: Duration::days(21),
            },
            action: StateAction::Move("Archive".to_string()),
            nerf: false,
        };

        // Unread message from 10 days ago (not past unread TTL of 21 days)
        let ten_days_ago = Utc::now() - Duration::days(10);
        let msg = make_test_message(&ten_days_ago.to_rfc3339(), vec![]); // no Seen flag
        let now = Utc::now();

        // Should NOT be expired - 10 days < 21 days unread TTL
        assert!(filter.evaluate_ttl(&msg, now).unwrap().is_none());
    }

    #[test]
    fn test_ttl_detailed_unread_message_expired() {
        let filter = StateFilter {
            name: "test".to_string(),
            labels: vec![],
            ttl: Ttl::Detailed {
                read: Duration::days(7),
                unread: Duration::days(21),
            },
            action: StateAction::Move("Archive".to_string()),
            nerf: false,
        };

        // Unread message from 25 days ago (past unread TTL of 21 days)
        let twenty_five_days_ago = Utc::now() - Duration::days(25);
        let msg = make_test_message(&twenty_five_days_ago.to_rfc3339(), vec![]);
        let now = Utc::now();

        let result = filter.evaluate_ttl(&msg, now).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn test_state_filter_matches_with_labels() {
        let filter = StateFilter {
            name: "test".to_string(),
            labels: vec![Label::Starred, Label::Important],
            ttl: Ttl::Keep,
            action: StateAction::Move("Archive".to_string()),
            nerf: false,
        };

        // Message with Starred label should match
        let msg_starred = make_test_message("2024-01-01T00:00:00+00:00", vec!["Starred"]);
        assert!(filter.matches(&msg_starred));

        // Message without matching labels should not match
        let msg_other = make_test_message("2024-01-01T00:00:00+00:00", vec!["INBOX"]);
        assert!(!filter.matches(&msg_other));
    }

    #[test]
    fn test_state_filter_empty_labels_matches_all() {
        let filter = StateFilter {
            name: "test".to_string(),
            labels: vec![], // empty = match all
            ttl: Ttl::Keep,
            action: StateAction::Move("Archive".to_string()),
            nerf: false,
        };

        let msg = make_test_message("2024-01-01T00:00:00+00:00", vec!["anything"]);
        assert!(filter.matches(&msg));
    }

    #[test]
    fn test_ttl_deserialize_keep() {
        let yaml = "Keep";
        let ttl: Ttl = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(ttl, Ttl::Keep);
    }

    #[test]
    fn test_ttl_deserialize_days() {
        let yaml = "7d";
        let ttl: Ttl = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(ttl, Ttl::Days(Duration::days(7)));
    }

    #[test]
    fn test_ttl_deserialize_detailed() {
        let yaml = "read: 7d\nunread: 21d";
        let ttl: Ttl = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            ttl,
            Ttl::Detailed {
                read: Duration::days(7),
                unread: Duration::days(21)
            }
        );
    }
}
