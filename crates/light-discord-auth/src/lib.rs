use anyhow::{anyhow, Result};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use password_hash::{PasswordHash, SaltString};
use rand_core::OsRng;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|err| anyhow!("failed to hash password: {err}"))?
        .to_string();
    Ok(hash)
}

pub fn verify_password(password: &str, password_hash: &str) -> bool {
    let Ok(parsed_hash) = PasswordHash::new(password_hash) else {
        return false;
    };

    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

pub fn new_invite_code() -> String {
    format!("invite_{}", Uuid::new_v4().simple())
}

pub fn new_session_token() -> String {
    format!("session_{}", Uuid::new_v4().simple())
}

pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_verifies_original_password() {
        let hash = hash_password("correct horse battery staple").unwrap();

        assert!(verify_password("correct horse battery staple", &hash));
        assert!(!verify_password("wrong", &hash));
    }

    #[test]
    fn generated_tokens_have_distinct_prefixes() {
        assert!(new_invite_code().starts_with("invite_"));
        assert!(new_session_token().starts_with("session_"));
        assert_ne!(new_invite_code(), new_invite_code());
    }

    #[test]
    fn token_hash_is_stable_without_storing_secret() {
        let token = "session_example";

        assert_eq!(hash_token(token), hash_token(token));
        assert_ne!(hash_token(token), token);
    }
}
