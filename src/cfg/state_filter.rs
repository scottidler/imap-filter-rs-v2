// src/cfg/state_filter.rs

use chrono::{DateTime, Utc};
use eyre::eyre;
use serde::Deserialize;
use serde::de::{self, Deserializer};
use serde_yaml::Value;
use chrono;

use crate::cfg::label::Label;
use crate::message::Message;
use crate::utils::parse_days;

#[derive(Clone, Debug, PartialEq)]
pub enum TTL {
    Keep,
    Days(chrono::Duration),
    Detailed { read: chrono::Duration, unread: chrono::Duration },
}

impl<'de> Deserialize<'de> for TTL {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct TTLVisitor;

        impl<'de> de::Visitor<'de> for TTLVisitor {
            type Value = TTL;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("Keep, '<n>d', or { read: '<n>d', unread: '<n>d' }")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if value == "Keep" {
                    Ok(TTL::Keep)
                } else {
                    parse_days(value)
                        .map(TTL::Days)
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
                            read = Some(
                                parse_days(&v).map_err(|e| de::Error::custom(e.to_string()))?
                            );
                        }
                        "unread" => {
                            let v: String = map.next_value()?;
                            unread = Some(
                                parse_days(&v).map_err(|e| de::Error::custom(e.to_string()))?
                            );
                        }
                        _ => return Err(de::Error::unknown_field(&key, &["read", "unread"])),
                    }
                }

                let read = read.ok_or_else(|| de::Error::missing_field("read"))?;
                let unread = unread.ok_or_else(|| de::Error::missing_field("unread"))?;
                Ok(TTL::Detailed { read, unread })
            }
        }

        deserializer.deserialize_any(TTLVisitor)
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
    pub ttl: TTL,

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
    pub fn evaluate_ttl(
        &self,
        msg: &Message,
        now: DateTime<Utc>,
    ) -> eyre::Result<Option<StateAction>> {
        // parse the stored RFC3339 date back into a chrono DateTime
        let internal: DateTime<Utc> = DateTime::parse_from_rfc3339(&msg.date)
            .map_err(|e| eyre!("Bad INTERNALDATE '{}': {}", msg.date, e))?
            .with_timezone(&Utc);

        let age = now.signed_duration_since(internal);

        let ttl_duration = match &self.ttl {
            TTL::Keep => return Ok(None),
            TTL::Days(dur) => *dur,
            TTL::Detailed { unread, .. } => *unread,
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
                other => Err(de::Error::unknown_field(&other, &["Move", "Delete"])),
            }
        }
        _ => Err(de::Error::custom("Invalid `action` value")),
    }
}

fn default_action() -> StateAction {
    StateAction::Move(String::new())
}
