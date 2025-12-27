// src/cfg/secure.rs

use secure_string::SecureString;
use serde::{Deserialize, Deserializer};

/// Deserializes an `Option<SecureString>` from YAML.
pub fn deserialize_opt<'de, D>(deserializer: D) -> Result<Option<SecureString>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(opt.map(SecureString::from))
}
