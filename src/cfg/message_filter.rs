// src/message_filter.rs

use serde::Deserialize;
use serde::de::{self, Deserializer};
use serde_yaml::Value;

use crate::cfg::label::Label;

#[derive(Debug, PartialEq, Deserialize)]
pub struct AddressFilter {
    pub patterns: Vec<String>,
}

#[derive(Debug, PartialEq, Deserialize)]
pub struct SubjectFilter {
    pub patterns: Vec<String>,
}

#[derive(Debug, PartialEq, Deserialize)]
pub enum FilterAction {
    Star,
    Flag,
    Move(String),
}

/// A little helper to deserialize the `labels:` section of your YAML.
///
/// - If absent entirely → both Vecs empty.
/// - If only `included` present → `labels_included` populated.
/// - If only `excluded` present → `labels_excluded` populated.
/// - If both present → both populated.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct LabelsFilter {
    pub included: Vec<Label>,
    pub excluded: Vec<Label>,
}

#[derive(Debug, Deserialize)]
pub struct MessageFilter {
    // the map-key
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

    // allow `label: "Foo"` or `labels: ["A","B"]` or the full map of included/excluded
    #[serde(default)]
    #[serde(alias = "label")]
    #[serde(deserialize_with = "deserialize_labels_filter")]
    pub labels: LabelsFilter,

    // allow `action: Star` or `actions: ["Star","Flag"]`
    #[serde(default)]
    #[serde(alias = "action")]
    #[serde(deserialize_with = "deserialize_actions")]
    pub actions: Vec<FilterAction>,
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
