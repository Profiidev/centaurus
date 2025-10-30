use argon2::{
  Argon2,
  password_hash::{PasswordHasher, SaltString},
};
#[cfg(feature = "axum")]
use axum::{Extension, extract::FromRequestParts};
use base64::prelude::*;
use rsa::{
  Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey, pkcs1::EncodeRsaPublicKey, pkcs8::LineEnding,
};
use tracing::instrument;

use crate::error::Result;

#[cfg(feature = "axum")]
#[derive(Clone, FromRequestParts)]
#[from_request(via(Extension))]
pub struct PasswordState {
  key: RsaPrivateKey,
  pub pub_key: String,
  pub pepper: Vec<u8>,
}

#[cfg(feature = "axum")]
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
