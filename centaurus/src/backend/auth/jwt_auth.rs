use std::sync::{Arc, atomic::AtomicI32};

use aide::OperationIo;
use axum::{Extension, extract::FromRequestParts};
use axum_extra::extract::cookie::{Cookie, SameSite};
use chrono::{Duration, Utc};
use jsonwebtoken::{
  Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode,
  errors::{Error, ErrorKind},
};
use rsa::{
  RsaPrivateKey, RsaPublicKey,
  pkcs1::{DecodeRsaPrivateKey, EncodeRsaPrivateKey, EncodeRsaPublicKey},
  pkcs8::LineEnding,
  rand_core::OsRng,
};
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

use crate::{
  backend::auth::settings::AuthConfig,
  db::{init::Connection, tables::ConnectionExt},
  error::Result,
};

pub const JWT_COOKIE_NAME: &str = "centaurus_jwt";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct JwtClaims {
  pub exp: i64,
  pub iss: String,
  pub sub: Uuid,
}

#[derive(Clone, FromRequestParts, OperationIo)]
#[from_request(via(Extension))]
pub struct JwtState {
  header: Header,
  encoding_key: EncodingKey,
  decoding_key: DecodingKey,
  validation: Validation,
  pub iss: String,
  pub exp: i64,
}

impl JwtState {
  pub fn create_raw_token(&self, uuid: Uuid) -> Result<String> {
    let exp = Utc::now()
      .checked_add_signed(Duration::seconds(self.exp))
      .ok_or(Error::from(ErrorKind::ExpiredSignature))?
      .timestamp();

    let claims = JwtClaims {
      exp,
      iss: self.iss.clone(),
      sub: uuid,
    };

    Ok(encode(&self.header, &claims, &self.encoding_key)?)
  }

  pub fn create_token<'c>(&self, uuid: Uuid) -> Result<Cookie<'c>> {
    let token = self.create_raw_token(uuid)?;
    Ok(self.create_cookie(JWT_COOKIE_NAME, token))
  }

  pub fn create_cookie<'c>(&self, name: &'static str, value: String) -> Cookie<'c> {
    Cookie::build((name, value))
      .http_only(true)
      .max_age(time::Duration::seconds(self.exp))
      .same_site(SameSite::Lax)
      .secure(true)
      .path("/")
      .build()
  }

  pub fn validate_token(&self, token: &str) -> std::result::Result<JwtClaims, Error> {
    Ok(decode::<JwtClaims>(token, &self.decoding_key, &self.validation)?.claims)
  }

  pub async fn init(config: &AuthConfig, db: &Connection) -> Self {
    let (key, kid) = if let Ok(key) = db.key().get_key_by_name("jwt".into()).await {
      (key.private_key, key.id.to_string())
    } else {
      let mut rng = OsRng {};
      info!("Generating new JWT RSA keypair. This may take a few seconds...");
      let private_key = RsaPrivateKey::new(&mut rng, 4096).expect("Failed to create Rsa key");
      let key = private_key
        .to_pkcs1_pem(LineEnding::CRLF)
        .expect("Failed to export private key")
        .to_string();

      let uuid = Uuid::new_v4();

      db.key()
        .create_key("jwt".into(), key.clone(), uuid)
        .await
        .expect("Failed to save key");

      (key, uuid.to_string())
    };

    let private_key = RsaPrivateKey::from_pkcs1_pem(&key).expect("Failed to load public key");
    let public_key = RsaPublicKey::from(private_key);
    let public_key_pem = public_key
      .to_pkcs1_pem(LineEnding::CRLF)
      .expect("Failed to export public key");

    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(kid.clone());

    let encoding_key =
      EncodingKey::from_rsa_pem(key.as_bytes()).expect("Failed to create encoding key");
    let decoding_key =
      DecodingKey::from_rsa_pem(public_key_pem.as_bytes()).expect("Failed to create decoding key");
    let mut validation = Validation::new(Algorithm::RS256);
    validation.validate_aud = false;

    Self {
      header,
      encoding_key,
      decoding_key,
      validation,
      iss: config.auth_issuer.clone(),
      exp: config.auth_jwt_expiration,
    }
  }
}

#[derive(FromRequestParts, Clone, Default, OperationIo)]
#[from_request(via(Extension))]
pub struct JwtInvalidState {
  pub count: Arc<AtomicI32>,
}
