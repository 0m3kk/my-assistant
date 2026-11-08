use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Nonce};
use base64::Engine;

const NONCE_LEN: usize = 12;

pub fn encrypt(
    master_key: &[u8; 32],
    plaintext: &str,
) -> crate::error::AppResult<(Vec<u8>, Vec<u8>)> {
    let cipher = Aes256Gcm::new(master_key.into());
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|_| crate::error::AppError::internal("encryption failure"))?;
    Ok((ciphertext, nonce.to_vec()))
}

pub fn decrypt(
    master_key: &[u8; 32],
    ciphertext: &[u8],
    nonce: &[u8],
) -> crate::error::AppResult<String> {
    if nonce.len() != NONCE_LEN {
        return Err(crate::error::AppError::internal("invalid nonce length"));
    }
    let cipher = Aes256Gcm::new(master_key.into());
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|_| crate::error::AppError::internal("decryption failure"))?;
    String::from_utf8(plaintext)
        .map_err(|_| crate::error::AppError::internal("decrypted value is not utf-8"))
}

/// Short non-revealing preview for admin UI: `sk-…f3a9`.
#[allow(dead_code)]
pub fn mask(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 8 {
        "…".to_string()
    } else {
        let head: String = chars[..2].iter().collect();
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{head}…{tail}")
    }
}

/// Encrypt with an additional base64 wrapper so the value can travel as JSON string if needed.
#[allow(dead_code)]
pub fn encrypt_b64(master_key: &[u8; 32], plaintext: &str) -> crate::error::AppResult<String> {
    let (ct, nonce) = encrypt(master_key, plaintext)?;
    let mut combined = nonce;
    combined.extend_from_slice(&ct);
    Ok(base64::engine::general_purpose::STANDARD.encode(combined))
}

#[allow(dead_code)]
pub fn decrypt_b64(master_key: &[u8; 32], combined_b64: &str) -> crate::error::AppResult<String> {
    let combined = base64::engine::general_purpose::STANDARD
        .decode(combined_b64)
        .map_err(|_| crate::error::AppError::internal("invalid base64"))?;
    if combined.len() <= NONCE_LEN {
        return Err(crate::error::AppError::internal("ciphertext too short"));
    }
    let (nonce, ct) = combined.split_at(NONCE_LEN);
    decrypt(master_key, ct, nonce)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [7u8; 32];

    #[test]
    fn roundtrip() {
        let (ct, nonce) = encrypt(&KEY, "hunter2").unwrap();
        assert_ne!(ct, b"hunter2");
        assert_eq!(decrypt(&KEY, &ct, &nonce).unwrap(), "hunter2");
    }

    #[test]
    fn wrong_key_fails() {
        let (ct, nonce) = encrypt(&KEY, "hunter2").unwrap();
        let wrong = [8u8; 32];
        assert!(decrypt(&wrong, &ct, &nonce).is_err());
    }

    #[test]
    fn b64_roundtrip() {
        let enc = encrypt_b64(&KEY, "value-123").unwrap();
        assert_eq!(decrypt_b64(&KEY, &enc).unwrap(), "value-123");
    }

    #[test]
    fn masking() {
        assert_eq!(mask("sk-abcdefghij1234"), "sk…1234");
        assert_eq!(mask("short"), "…");
    }
}
