//! Random tokens, one-time codes and password hashing.

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{Rng, RngCore};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// A new opaque session token (32 random bytes, base64url) and its SHA-256 for storage.
pub fn new_session_token() -> (String, Vec<u8>) {
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let token = URL_SAFE_NO_PAD.encode(bytes);
    let hash = hash_token(&token);
    (token, hash)
}

/// SHA-256 of a session token, as stored in `sessions.token_hash`.
pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// A uniformly random 6-digit code.
pub fn new_code() -> String {
    format!("{:06}", rand::rng().random_range(0..1_000_000_u32))
}

/// Hash of a one-time code, salted with its row id.
pub fn hash_code(code_id: Uuid, code: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(code_id.as_bytes());
    hasher.update(code.trim().as_bytes());
    hasher.finalize().to_vec()
}

/// Constant-time byte comparison.
pub fn ct_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0_u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Hashes a password with argon2id (default parameters). CPU-heavy: call off the async runtime.
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| anyhow::anyhow!("hashing password: {err}"))
}

/// Verifies a password against a stored PHC hash. CPU-heavy: call off the async runtime.
pub fn verify_password(password: &str, phc: &str) -> bool {
    PasswordHash::new(phc).is_ok_and(|parsed| {
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique_and_hash_stably() {
        let (t1, h1) = new_session_token();
        let (t2, _) = new_session_token();
        assert_ne!(t1, t2);
        assert_eq!(t1.len(), 43);
        assert_eq!(hash_token(&t1), h1);
    }

    #[test]
    fn codes_are_six_digits() {
        for _ in 0..100 {
            let code = new_code();
            assert_eq!(code.len(), 6);
            assert!(code.chars().all(|ch| ch.is_ascii_digit()));
        }
    }

    #[test]
    fn code_hash_depends_on_id() {
        let (first, second) = (Uuid::now_v7(), Uuid::now_v7());
        assert_ne!(hash_code(first, "123456"), hash_code(second, "123456"));
        assert!(ct_eq(
            &hash_code(first, "123456"),
            &hash_code(first, " 123456 ")
        ));
    }

    #[test]
    fn password_roundtrip() {
        let phc = hash_password("correct horse battery").unwrap();
        assert!(phc.starts_with("$argon2id$"));
        assert!(verify_password("correct horse battery", &phc));
        assert!(!verify_password("wrong", &phc));
        assert!(!verify_password("x", "not a hash"));
    }
}
