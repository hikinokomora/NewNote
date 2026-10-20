//! AES-256-GCM encryption with Argon2id key derivation.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::RngCore;

use crate::model::LockInfo;

const VERIFIER: &[u8] = b"newnote-key-check";
const MAGIC: &[u8; 4] = b"NNE1";

#[derive(Clone)]
pub struct Key(pub [u8; 32]);

impl Drop for Key {
    fn drop(&mut self) {
        // Wipe key material when it goes out of scope.
        for b in self.0.iter_mut() {
            unsafe { std::ptr::write_volatile(b, 0) };
        }
    }
}

pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

pub fn derive_key(password: &str, salt: &[u8]) -> Key {
    let mut out = [0u8; 32];
    let params = argon2::Params::new(64 * 1024, 3, 1, Some(32)).expect("argon2 params");
    let a = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    a.hash_password_into(password.as_bytes(), salt, &mut out)
        .expect("argon2");
    Key(out)
}

pub fn encrypt(key: &Key, plain: &[u8]) -> Vec<u8> {
    let cipher = Aes256Gcm::new_from_slice(&key.0).expect("key");
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plain)
        .expect("encrypt");
    let mut out = Vec::with_capacity(4 + 12 + ct.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    out
}

pub fn decrypt(key: &Key, data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 16 || &data[..4] != MAGIC {
        return None;
    }
    let cipher = Aes256Gcm::new_from_slice(&key.0).ok()?;
    cipher
        .decrypt(Nonce::from_slice(&data[4..16]), &data[16..])
        .ok()
}

pub fn is_encrypted(data: &[u8]) -> bool {
    data.len() >= 4 && &data[..4] == MAGIC
}

/// Creates protection info for a new password and returns the derived key.
pub fn new_lock(password: &str) -> (LockInfo, Key) {
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    let key = derive_key(password, &salt);
    let verifier = encrypt(&key, VERIFIER);
    (
        LockInfo {
            salt: hex(&salt),
            verifier: hex(&verifier),
            no_copy: false,
        },
        key,
    )
}

/// Checks the password, returns the key on success.
pub fn unlock(info: &LockInfo, password: &str) -> Option<Key> {
    let salt = unhex(&info.salt)?;
    let key = derive_key(password, &salt);
    let v = decrypt(&key, &unhex(&info.verifier)?)?;
    (v == VERIFIER).then_some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let (info, key) = new_lock("пароль");
        let ct = encrypt(&key, b"hello");
        assert_eq!(decrypt(&key, &ct).unwrap(), b"hello");
        assert!(unlock(&info, "пароль").is_some());
        assert!(unlock(&info, "wrong").is_none());
    }
}
