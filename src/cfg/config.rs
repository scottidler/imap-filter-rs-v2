// src/cfg/config.rs

use eyre::{eyre, Result};
use log::{debug, error};
use secure_string::SecureString;
use serde::de::{self, Deserializer};
use serde::Deserialize;
use serde_yaml::{from_value, Value};
use std::fs;
use std::path::Path;

use crate::cfg::message_filter::MessageFilter;
use crate::cfg::secure;
use crate::cfg::state_filter::StateFilter;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(alias = "imap-domain")]
    pub imap_domain: Option<String>,

    #[serde(alias = "imap-username")]
    pub imap_username: Option<String>,

    #[serde(alias = "imap-password", default, deserialize_with = "secure::deserialize_opt")]
    pub imap_password: Option<SecureString>,

    // OAuth2 authentication (alternative to password)
    #[serde(alias = "oauth2-client-id", default, deserialize_with = "secure::deserialize_opt")]
    pub oauth2_client_id: Option<SecureString>,

    #[serde(
        alias = "oauth2-client-secret",
        default,
        deserialize_with = "secure::deserialize_opt"
    )]
    pub oauth2_client_secret: Option<SecureString>,

    #[serde(
        alias = "oauth2-refresh-token",
        default,
        deserialize_with = "secure::deserialize_opt"
    )]
    pub oauth2_refresh_token: Option<SecureString>,

    /// flatten name + body into Vec<MessageFilter>
    #[serde(rename = "message-filters")]
    #[serde(deserialize_with = "deserialize_named_filters")]
    pub message_filters: Vec<MessageFilter>,

    /// flatten name + body into Vec<StateFilter>
    #[serde(rename = "state-filters")]
    #[serde(deserialize_with = "deserialize_named_states")]
    pub state_filters: Vec<StateFilter>,
}

pub fn load_config(config_path: &Path) -> Result<Config> {
    debug!("Loading configuration from {:?}", config_path);

    let content = fs::read_to_string(config_path).map_err(|e| {
        error!("Failed to read config file {}: {}", config_path.display(), e);
        eyre!("Failed to read config file {}: {}", config_path.display(), e)
    })?;

    let cfg: Config = serde_yaml::from_str(&content).map_err(|e| {
        error!("Failed to parse YAML: {}", e);
        eyre!("Failed to parse YAML: {}", e)
    })?;

    debug!("Successfully loaded configuration");
    Ok(cfg)
}

fn deserialize_named_filters<'de, D>(deserializer: D) -> Result<Vec<MessageFilter>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(deserializer).map_err(de::Error::custom)?;
    let seq = match v {
        Value::Sequence(s) => s,
        _ => return Err(de::Error::custom("`filters` must be a sequence")),
    };
    let mut out = Vec::new();
    for entry in seq {
        if let Value::Mapping(map) = entry {
            if map.len() != 1 {
                return Err(de::Error::custom("Each filter must have exactly one name→body"));
            }
            let (k, v) = map.into_iter().next().unwrap();
            let name = match k {
                Value::String(s) => s,
                _ => return Err(de::Error::custom("Filter name must be a string")),
            };
            let mut filt: MessageFilter = from_value(v).map_err(de::Error::custom)?;
            filt.name = name.clone();
            out.push(filt);
        } else {
            return Err(de::Error::custom("Invalid entry in filters list"));
        }
    }
    Ok(out)
}

fn deserialize_named_states<'de, D>(deserializer: D) -> Result<Vec<StateFilter>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(deserializer).map_err(de::Error::custom)?;
    let seq = match v {
        Value::Sequence(s) => s,
        _ => return Err(de::Error::custom("`states` must be a sequence")),
    };
    let mut out = Vec::new();
    for entry in seq {
        if let Value::Mapping(map) = entry {
            if map.len() != 1 {
                return Err(de::Error::custom("Each state must have exactly one name→body"));
            }
            let (k, v) = map.into_iter().next().unwrap();
            let name = match k {
                Value::String(s) => s,
                _ => return Err(de::Error::custom("State name must be a string")),
            };
            let mut st: StateFilter = from_value(v).map_err(de::Error::custom)?;
            st.name = name.clone();
            out.push(st);
        } else {
            return Err(de::Error::custom("Invalid entry in states list"));
        }
    }
    Ok(out)
}
