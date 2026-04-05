use argon2::{
  Argon2,
  password_hash::{PasswordHasher, SaltString},
};
use axum::{Extension, extract::FromRequestParts};
use base64::prelude::*;
use rsa::{
  Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey,
  pkcs1::{DecodeRsaPrivateKey, EncodeRsaPrivateKey, EncodeRsaPublicKey},
  pkcs8::LineEnding,
  rand_core::OsRng,
};
use tracing::{info, instrument};
use uuid::Uuid;

use crate::{
  backend::auth::settings::AuthConfig,
  db::{init::Connection, tables::ConnectionExt},
  error::Result,
};

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

pub async fn init_pw_state(config: &AuthConfig, db: &Connection) -> PasswordState {
  let key = if let Ok(key) = db.key().get_key_by_name("password".into()).await {
    RsaPrivateKey::from_pkcs1_pem(&key.private_key).expect("Failed to parse private password key")
  } else {
    let mut rng = OsRng {};
    info!(
      "Generating new RSA key for password transfer encryption. This may take a few seconds..."
    );
    let private_key = RsaPrivateKey::new(&mut rng, 4096).expect("Failed to create Rsa key");
    let key = private_key
      .to_pkcs1_pem(LineEnding::CRLF)
      .expect("Failed to export private key")
      .to_string();

    db.key()
      .create_key("password".into(), key.clone(), Uuid::new_v4())
      .await
      .expect("Failed to save key");

    private_key
  };

  let pepper = config.auth_pepper.as_bytes().to_vec();
  PasswordState::init(pepper, key).await
}
