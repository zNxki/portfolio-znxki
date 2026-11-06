use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

type HmacSha256 = Hmac<Sha256>;

pub fn random_token() -> String {
    let mut buf = [0u8; 32];
    OsRng.fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

pub fn sha256(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(sha256(data))
}

pub fn hash_token(token: &str) -> Vec<u8> {
    sha256(token.as_bytes())
}

pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(sha256(verifier.as_bytes()))
}

pub fn csrf_token(secret: &[u8], session_id: &Uuid) -> AppResult<String> {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(secret)
        .map_err(|_| AppError::internal("chiave HMAC non valida"))?;
    mac.update(b"csrf-v1:");
    mac.update(session_id.as_bytes());
    Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique_and_long_enough() {
        let a = random_token();
        let b = random_token();
        assert_ne!(a, b);
        assert!(a.len() >= 43);
    }

    #[test]
    fn pkce_matches_rfc7636_vector() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            pkce_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn hash_is_deterministic_and_not_plain() {
        let h = hash_token("abc");
        assert_eq!(h, hash_token("abc"));
        assert_eq!(h.len(), 32);
        assert_ne!(h, b"abc".to_vec());
    }

    #[test]
    fn csrf_is_bound_to_session_and_secret() {
        let s1 = Uuid::new_v4();
        let s2 = Uuid::new_v4();
        let secret = b"0123456789abcdef0123456789abcdef";
        let a = csrf_token(secret, &s1).unwrap_or_default();
        let b = csrf_token(secret, &s2).unwrap_or_default();
        let c = csrf_token(b"another-secret-another-secret-123", &s1).unwrap_or_default();
        assert!(!a.is_empty());
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a, csrf_token(secret, &s1).unwrap_or_default());
    }

    #[test]
    fn constant_time_compare() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"abcd"));
    }
}