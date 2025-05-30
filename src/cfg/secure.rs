// src/cfg/secure.rs

use serde::{Deserialize, Deserializer};
use secure_string::SecureString;

/// Deserializes a `SecureString` from a plain string in YAML.
pub fn deserialize<'de, D>(deserializer: D) -> Result<SecureString, D::Error>
where
    D: Deserializer<'de>,
{
    let plain = String::deserialize(deserializer)?;
    Ok(SecureString::from(plain))
}

/// Deserializes an `Option<SecureString>` from YAML.
pub fn deserialize_opt<'de, D>(deserializer: D) -> Result<Option<SecureString>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(opt.map(SecureString::from))
}
