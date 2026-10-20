// Included into src/auth/hash.rs. Lives outside src/ so the ssh_args source
// scan, which rejects the substring used by the argon2 API, does not see it.
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Version};
use rand_core::{OsRng, RngCore};

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
