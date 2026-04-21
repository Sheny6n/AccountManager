use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;

pub const SALT_LEN: usize = 16;
pub const KEY_LEN: usize = 32;

pub type Key = [u8; KEY_LEN];

pub fn generate_salt() -> [u8; SALT_LEN] {
    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    salt
}

pub fn derive_key(password: &str, salt: &[u8]) -> Result<Key, String> {
    let params = Params::new(64 * 1024, 3, 4, Some(KEY_LEN))
        .map_err(|e| format!("argon2 params: {e}"))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; KEY_LEN];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| format!("argon2 derive: {e}"))?;
    Ok(key)
}

pub fn key_to_hex(key: &[u8]) -> String {
    let mut s = String::with_capacity(key.len() * 2);
    for b in key {
        s.push_str(&format!("{:02x}", b));
    }
    s
}
