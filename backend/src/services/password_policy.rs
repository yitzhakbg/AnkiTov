//! Password policy for the Prong 8 first-login wizard.
//!
//! Pure and unit-testable — no DB, no I/O. The first-login handler calls
//! [`check`] before `hash_password`.

use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum PolicyError {
    #[error("password too short")]
    TooShort,
    #[error("password must contain a digit")]
    NoDigit,
    #[error("password must not equal the email address")]
    EqualsEmail(String),
    #[error("password found in a dictionary")]
    InDictionary,
}

/// Pure validation. v1 enforces: length >= 8, at least one digit,
/// and (when email is provided) not equal to the email or its local part.
/// The dictionary check is a no-op in v1 (the gate is left in place).
pub fn check(candidate: &str, email: Option<&str>) -> Result<(), PolicyError> {
    if candidate.len() < 8 {
        return Err(PolicyError::TooShort);
    }
    if !candidate.chars().any(|c| c.is_ascii_digit()) {
        return Err(PolicyError::NoDigit);
    }
    if let Some(email) = email {
        let local = email.split('@').next().unwrap_or(email).to_lowercase();
        let cand = candidate.to_lowercase();
        if cand == email.to_lowercase() || (cand == local && !local.is_empty()) {
            return Err(PolicyError::EqualsEmail(email.to_string()));
        }
    }
    // InDictionary: intentionally skipped in v1 (future prong).
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn too_short_rejected() {
        // 7 chars, has a digit
        assert_eq!(check("a1bcdef", None), Err(PolicyError::TooShort));
    }

    #[test]
    fn no_digit_rejected() {
        // 8 chars, all alpha
        assert_eq!(check("abcdefgh", None), Err(PolicyError::NoDigit));
    }

    #[test]
    fn valid_accepted() {
        // 8 chars with a digit
        assert_eq!(check("abc1defg", None), Ok(()));
    }

    #[test]
    fn equals_email_rejected() {
        // candidate == email exactly (16 chars, contains a digit)
        let email = "jane2doe@school.edu";
        assert_eq!(
            check(email, Some(email)),
            Err(PolicyError::EqualsEmail(email.to_string()))
        );
    }
}
