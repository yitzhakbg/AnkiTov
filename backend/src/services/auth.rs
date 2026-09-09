//! JWT authentication utilities.
//!
//! Provides token encoding/decoding and the `Claims` struct used by the auth
//! middleware to identify the requesting user.

use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

/// JWT secret — MUST be set via ANKITOV_JWT_SECRET env var in production.
/// Falls back to a dev-only default for local development.
fn jwt_secret() -> String {
    std::env::var("ANKITOV_JWT_SECRET")
        .unwrap_or_else(|_| "ankitov-dev-secret-change-in-production".to_string())
}

/// Claims embedded in every JWT token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// User ID from the `users` table.
    pub sub: i64,
    /// User role: "student", "teacher", "admin".
    pub role: String,
    /// Email address.
    pub email: String,
    /// Expiration timestamp (Unix epoch seconds).
    pub exp: usize,
    /// Issued-at timestamp (Unix epoch seconds).
    pub iat: usize,
}

/// Encode a JWT token for the given user.
///
/// Tokens expire after 24 hours (configurable via `expire_secs`).
pub fn encode_token(user_id: i64, role: &str, email: &str, expire_secs: usize) -> Result<String, jsonwebtoken::errors::Error> {
    let now = chrono::Utc::now().timestamp() as usize;
    let claims = Claims {
        sub: user_id,
        role: role.to_string(),
        email: email.to_string(),
        exp: now + expire_secs,
        iat: now,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret().as_ref()),
    )
}

/// Decode and validate a JWT token.
///
/// Returns `Err` if the token is expired, malformed, or has an invalid
/// signature.
pub fn decode_token(token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret().as_ref()),
        &Validation::default(),
    )?;
    Ok(token_data.claims)
}

/// The hard-coded development-only JWT secret.
///
/// Used by [`is_default_secret`] to detect when a real
/// `ANKITOV_JWT_SECRET` has not been configured.
pub const DEV_SECRET: &str = "ankitov-dev-secret-change-in-production";

/// Whether the active JWT secret is the insecure development default.
///
/// `true` when `ANKITOV_JWT_SECRET` is unset, empty, or exactly equal to the
/// hard-coded [`DEV_SECRET`]. Used by the startup boot guard (see
/// `app.rs`) to refuse to boot in production with an unconfigured secret and
/// to warn in development / test.
pub fn is_default_secret() -> bool {
    match std::env::var("ANKITOV_JWT_SECRET") {
        Ok(v) if !v.trim().is_empty() && v != DEV_SECRET => false,
        _ => true,
    }
}

/// Hash a password with bcrypt (cost factor 12).
pub fn hash_password(password: &str) -> Result<String, bcrypt::BcryptError> {
    bcrypt::hash(password, 12)
}

/// Verify a password against a bcrypt hash.
pub fn verify_password(password: &str, hash: &str) -> Result<bool, bcrypt::BcryptError> {
    bcrypt::verify(password, hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let token = encode_token(42, "student", "alice@example.com", 3600)
            .expect("encode");
        let claims = decode_token(&token).expect("decode");
        assert_eq!(claims.sub, 42);
        assert_eq!(claims.role, "student");
        assert_eq!(claims.email, "alice@example.com");
    }

    #[test]
    fn test_expired_token_fails() {
        // Manually construct a token that expired 1 hour ago
        use jsonwebtoken::{encode, EncodingKey, Header};
        let claims = Claims {
            sub: 42,
            role: "student".into(),
            email: "alice@example.com".into(),
            exp: chrono::Utc::now().timestamp() as usize - 3600,
            iat: chrono::Utc::now().timestamp() as usize - 7200,
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(jwt_secret().as_ref()),
        )
        .expect("encode");
        let result = decode_token(&token);
        assert!(result.is_err(), "expired token should fail validation");
    }

    #[test]
    fn test_password_hash_verify() {
        let hash = hash_password("test1234").expect("hash");
        assert!(verify_password("test1234", &hash).expect("verify"));
        assert!(!verify_password("wrong", &hash).expect("verify wrong"));
    }
}