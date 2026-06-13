use argon2::{
  Argon2,
  password_hash::{PasswordHasher, SaltString},
};
use axum::{Extension, extract::FromRequestParts};
use base64::prelude::*;
use rsa::{
  Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey, pkcs1::EncodeRsaPublicKey, pkcs8::LineEnding,
};
use tracing::instrument;

use crate::error::Result;

#[derive(Clone, FromRequestParts)]
#[cfg_attr(feature = "openapi", derive(aide::OperationIo))]
#[from_request(via(Extension))]
pub struct PasswordState {
  key: RsaPrivateKey,
  pub pub_key: String,
  pub pepper: Vec<u8>,
}

impl PasswordState {
  pub fn decrypt(&self, message: &[u8]) -> Result<Vec<u8>> {
    Ok(self.key.decrypt(Pkcs1v15Encrypt, message)?)
  }

  #[instrument(skip(self, password))]
  pub fn pw_hash(&self, salt: &str, password: &str) -> Result<String> {
    use http::StatusCode;

    use crate::error::ErrorReportStatusExt;

    let bytes = BASE64_STANDARD
      .decode(password)
      .status(StatusCode::BAD_REQUEST)?;
    let pw_bytes = self
      .key
      .decrypt(Pkcs1v15Encrypt, &bytes)
      .status(StatusCode::BAD_REQUEST)?;
    let password = String::from_utf8_lossy(&pw_bytes).to_string();

    self.pw_hash_raw(salt, &password)
  }

  #[instrument(skip(self, password))]
  pub fn pw_hash_raw(&self, salt: &str, password: &str) -> Result<String> {
    hash_secret(&self.pepper, salt, password.as_bytes())
  }

  pub async fn init(pepper: Vec<u8>, key: RsaPrivateKey) -> Self {
    let pub_key = RsaPublicKey::from(&key)
      .to_pkcs1_pem(LineEnding::CRLF)
      .expect("Failed to export Rsa Public Key");

    if pepper.len() > 32 {
      panic!("Pepper is longer than 32 characters");
    }

    Self {
      key,
      pub_key,
      pepper,
    }
  }
}

pub fn hash_secret(pepper: &[u8], salt: &str, passphrase: &[u8]) -> Result<String> {
  let mut salt = BASE64_STANDARD_NO_PAD.decode(salt)?;
  salt.extend_from_slice(pepper);
  let salt_string = SaltString::encode_b64(&salt)?;

  let argon2 = Argon2::default();
  Ok(
    argon2
      .hash_password(passphrase, salt_string.as_salt())?
      .to_string(),
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use rsa::rand_core::OsRng;

  #[tokio::test]
  async fn test_pw_state_init() {
    let mut rng = OsRng;
    let key = RsaPrivateKey::new(&mut rng, 512).unwrap();
    let state = PasswordState::init(vec![1, 2, 3], key).await;
    assert!(state.pub_key.contains("BEGIN RSA PUBLIC KEY"));
  }

  #[test]
  fn test_hash_secret() {
    let hash = hash_secret(&[1, 2, 3], "c2FsdHNhbHQ", b"password").unwrap(); // "saltsalt" in base64
    assert!(hash.starts_with("$argon2id$"));
  }

  #[test]
  fn test_hash_secret_is_deterministic() {
    // The same pepper/salt/passphrase always produces the same hash, while a
    // different pepper produces a different one.
    let a = hash_secret(&[1, 2, 3], "c2FsdHNhbHQ", b"password").unwrap();
    let b = hash_secret(&[1, 2, 3], "c2FsdHNhbHQ", b"password").unwrap();
    assert_eq!(a, b);

    let c = hash_secret(&[9, 9, 9], "c2FsdHNhbHQ", b"password").unwrap();
    assert_ne!(a, c);
  }

  #[tokio::test]
  async fn test_decrypt_roundtrip() {
    let mut rng = OsRng;
    let key = RsaPrivateKey::new(&mut rng, 512).unwrap();
    let pub_key = RsaPublicKey::from(&key);
    let state = PasswordState::init(vec![1, 2, 3], key).await;

    let ciphertext = pub_key
      .encrypt(&mut rng, Pkcs1v15Encrypt, b"secret-message")
      .unwrap();
    assert_eq!(state.decrypt(&ciphertext).unwrap(), b"secret-message");
  }

  #[tokio::test]
  async fn test_pw_hash_matches_raw() {
    let mut rng = OsRng;
    let key = RsaPrivateKey::new(&mut rng, 512).unwrap();
    let pub_key = RsaPublicKey::from(&key);
    let state = PasswordState::init(vec![7], key).await;

    // pw_hash decrypts a base64'd RSA-encrypted password; the resulting hash
    // must equal hashing the plaintext directly.
    let ciphertext = pub_key
      .encrypt(&mut rng, Pkcs1v15Encrypt, b"hunter2")
      .unwrap();
    let encoded = BASE64_STANDARD.encode(&ciphertext);

    let from_encrypted = state.pw_hash("c2FsdHNhbHQ", &encoded).unwrap();
    let from_raw = state.pw_hash_raw("c2FsdHNhbHQ", "hunter2").unwrap();
    assert_eq!(from_encrypted, from_raw);
  }

  #[tokio::test]
  async fn test_pw_hash_rejects_invalid_base64() {
    let mut rng = OsRng;
    let key = RsaPrivateKey::new(&mut rng, 512).unwrap();
    let state = PasswordState::init(vec![1], key).await;
    assert!(state.pw_hash("c2FsdHNhbHQ", "!!!not base64!!!").is_err());
  }
}
