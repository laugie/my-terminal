//! argon2id hashing.
//!
//! Production cost is the OWASP recommendation: 19 MiB, time 2, parallelism 1,
//! and a random 16-byte salt stored inside the PHC string. Contract tests go
//! through [`HashParams::fast`] so debug builds stay quick.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use rand_core::{OsRng, RngCore};

/// Random salt length, in bytes. This is also Argon2's recommended salt length.
pub const SALT_LEN: usize = 16;

/// Memory, time, and parallelism for one argon2id hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashParams {
    pub memory_kib: u32,
    pub time_cost: u32,
    pub parallelism: u32,
}

impl HashParams {
    /// OWASP argon2id parameters. The running app hashes with these.
    pub const fn production() -> Self {
        Self {
            memory_kib: 19 * 1024,
            time_cost: 2,
            parallelism: 1,
        }
    }

    /// Tiny parameters for tests. Still argon2id, still a random salt.
    pub const fn fast() -> Self {
        Self {
            memory_kib: 8,
            time_cost: 1,
            parallelism: 1,
        }
    }

    pub fn to_argon(self) -> Result<Params, argon2::Error> {
        Params::new(self.memory_kib, self.time_cost, self.parallelism, None)
    }
}

pub(crate) fn hash_secret(params: HashParams, secret: &str) -> Result<String, ()> {
    let hasher = Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        params.to_argon().map_err(|_| ())?,
    );
    let mut salt_bytes = [0u8; SALT_LEN];
    let mut rng = OsRng;
    rng.fill_bytes(&mut salt_bytes);
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|_| ())?;
    let hashed = hasher
        .hash_password(secret.as_bytes(), &salt)
        .map_err(|_| ())?;
    Ok(hashed.to_string())
}

pub(crate) fn verify_secret(phc: &str, secret: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(phc) else {
        return false;
    };
    Argon2::default()
        .verify_password(secret.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime_secret() -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        format!("s-{nanos:x}-{:x}", std::process::id())
    }

    #[test]
    fn production_parameters_are_owasp_argon2id() {
        let params = HashParams::production();
        assert_eq!(params.memory_kib, 19 * 1024);
        assert_eq!(params.time_cost, 2);
        assert_eq!(params.parallelism, 1);
        assert_eq!(SALT_LEN, 16);
        assert_ne!(HashParams::fast(), params);

        let built = params.to_argon().expect("argon params");
        assert_eq!(built.m_cost(), 19 * 1024);
        assert_eq!(built.t_cost(), 2);
        assert_eq!(built.p_cost(), 1);

        let phc = hash_secret(params, &runtime_secret()).expect("production hash");
        assert!(phc.starts_with("$argon2id$"), "{phc}");
        assert!(phc.contains("m=19456,t=2,p=1"), "{phc}");
        let salt = phc.split('$').nth(4).expect("salt segment");
        assert_eq!(
            salt.len(),
            22,
            "16 random bytes encode to 22 unpadded base64 characters, got {salt}"
        );
    }
}
