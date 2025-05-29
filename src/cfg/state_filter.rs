// src/state.rs

use serde::Deserialize;
use serde::de::{self, Deserializer};
use serde_yaml::Value;

use crate::cfg::label::Label;

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum TTL {
    Keep,
    Simple(String),
    Detailed { read: String, unread: String },
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
