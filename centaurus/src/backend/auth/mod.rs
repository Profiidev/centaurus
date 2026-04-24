#[cfg(feature = "endpoints")]
use axum::Extension;
#[cfg(feature = "endpoints")]
use rsa::{
  RsaPrivateKey,
  pkcs1::{DecodeRsaPrivateKey, EncodeRsaPrivateKey},
  pkcs8::LineEnding,
  rand_core::OsRng,
};
#[cfg(feature = "endpoints")]
use tracing::info;
#[cfg(feature = "endpoints")]
use uuid::Uuid;

#[cfg(feature = "endpoints")]
use crate::{
  backend::{
    BackendRouter,
    auth::{
      jwt_state::{JwtInvalidState, JwtState},
      pw_state::PasswordState,
      settings::AuthConfig,
    },
    middleware::rate_limiter::RateLimiter,
  },
  db::{init::Connection, tables::ConnectionExt},
};

#[cfg(feature = "endpoints")]
pub mod config;
pub mod jwt;
#[cfg(feature = "endpoints")]
pub mod jwt_auth;
#[cfg(feature = "endpoints")]
pub mod jwt_state;
#[cfg(feature = "endpoints")]
pub mod logout;
#[cfg(feature = "endpoints")]
pub mod oidc;
#[cfg(feature = "endpoints")]
pub mod password;
#[cfg(feature = "endpoints")]
pub mod permission;
pub mod pw_state;
pub mod settings;
#[cfg(feature = "endpoints")]
pub mod test_token;

#[cfg(feature = "endpoints")]
pub fn router(rate_limiter: &mut RateLimiter) -> BackendRouter {
  let router = BackendRouter::new()
    .nest("/password", password::router(rate_limiter))
    .nest("/logout", logout::router())
    .nest("/test_token", test_token::router());

  #[cfg(feature = "avatar")]
  {
    router
      .nest("/oidc", oidc::router(rate_limiter))
      .nest("/config", config::router())
  }
  #[cfg(not(feature = "avatar"))]
  router
}

#[cfg(feature = "endpoints")]
pub async fn state(router: BackendRouter, config: &AuthConfig, db: &Connection) -> BackendRouter {
  #[cfg(feature = "avatar")]
  use crate::backend::auth::oidc::OidcState;

  let pw_state = init_pw_state(config, db).await;
  let jwt_state = JwtState::init(config, db).await;
  #[cfg(feature = "avatar")]
  let oidc_state = OidcState::new(db).await;

  let router = router
    .layer(Extension(pw_state))
    .layer(Extension(jwt_state))
    .layer(Extension(JwtInvalidState::default()));

  #[cfg(feature = "avatar")]
  {
    router.layer(Extension(oidc_state))
  }
  #[cfg(not(feature = "avatar"))]
  router
}

#[cfg(feature = "endpoints")]
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
