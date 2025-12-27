// src/oauth2.rs

use base64::{engine::general_purpose::STANDARD, Engine};
use eyre::{eyre, Result};
use log::{debug, info};
use serde::Deserialize;

/// OAuth2 credentials for Gmail IMAP authentication.
#[derive(Debug, Clone)]
pub struct OAuth2Credentials {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
}

/// Response from Google's token refresh endpoint.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    token_type: String,
}

impl OAuth2Credentials {
    /// Refresh the access token using the refresh token.
    pub fn refresh_access_token(&self) -> Result<String> {
        info!("Refreshing OAuth2 access token");

        let response = ureq::post("https://oauth2.googleapis.com/token")
            .send_form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("refresh_token", self.refresh_token.as_str()),
                ("grant_type", "refresh_token"),
            ])
            .map_err(|e| eyre!("Failed to refresh OAuth2 token: {}", e))?;

        let token_response: TokenResponse = response
            .into_json()
            .map_err(|e| eyre!("Failed to parse token response: {}", e))?;

        debug!(
            "Got new {} access token (expires in {} seconds)",
            token_response.token_type, token_response.expires_in
        );

        Ok(token_response.access_token)
    }
}

/// Build the XOAUTH2 authentication string for IMAP.
///
/// Format: base64("user=" + email + "\x01auth=Bearer " + access_token + "\x01\x01")
pub fn build_xoauth2_string(email: &str, access_token: &str) -> String {
    let auth_string = format!("user={}\x01auth=Bearer {}\x01\x01", email, access_token);
    STANDARD.encode(auth_string.as_bytes())
}

/// XOAUTH2 authenticator for the imap crate.
pub struct XOAuth2Authenticator {
    response: String,
}

impl XOAuth2Authenticator {
    pub fn new(email: &str, access_token: &str) -> Self {
        Self {
            response: build_xoauth2_string(email, access_token),
        }
    }
}

impl imap::Authenticator for XOAuth2Authenticator {
    type Response = String;

    fn process(&self, _challenge: &[u8]) -> Self::Response {
        self.response.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use imap::Authenticator;

    #[test]
    fn test_build_xoauth2_string() {
        let result = build_xoauth2_string("user@example.com", "access_token_123");
        // Decode and verify the format
        let decoded = STANDARD.decode(&result).unwrap();
        let decoded_str = String::from_utf8(decoded).unwrap();
        assert_eq!(
            decoded_str,
            "user=user@example.com\x01auth=Bearer access_token_123\x01\x01"
        );
    }

    #[test]
    fn test_xoauth2_authenticator() {
        let auth = XOAuth2Authenticator::new("test@gmail.com", "token123");
        let response = auth.process(b"");
        // Should return base64 encoded XOAUTH2 string
        let decoded = STANDARD.decode(&response).unwrap();
        let decoded_str = String::from_utf8(decoded).unwrap();
        assert!(decoded_str.starts_with("user=test@gmail.com"));
        assert!(decoded_str.contains("auth=Bearer token123"));
    }
}
