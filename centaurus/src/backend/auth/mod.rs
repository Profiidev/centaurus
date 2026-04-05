#[cfg(feature = "sea-orm")]
use axum::Extension;
#[cfg(feature = "sea-orm")]
use rsa::{
  RsaPrivateKey,
  pkcs1::{DecodeRsaPrivateKey, EncodeRsaPrivateKey},
  pkcs8::LineEnding,
  rand_core::OsRng,
};
#[cfg(feature = "sea-orm")]
use tracing::info;
#[cfg(feature = "sea-orm")]
use uuid::Uuid;

#[cfg(feature = "sea-orm")]
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

#[cfg(all(feature = "sea-orm", feature = "image", feature = "gravatar"))]
pub mod config;
pub mod jwt;
#[cfg(feature = "sea-orm")]
pub mod jwt_auth;
#[cfg(feature = "sea-orm")]
pub mod jwt_state;
#[cfg(feature = "sea-orm")]
pub mod logout;
#[cfg(all(feature = "sea-orm", feature = "image", feature = "gravatar"))]
pub mod oidc;
#[cfg(feature = "sea-orm")]
pub mod password;
#[cfg(feature = "sea-orm")]
pub mod permission;
pub mod pw_state;
pub mod settings;
#[cfg(feature = "sea-orm")]
pub mod test_token;

#[cfg(feature = "sea-orm")]
pub fn router(rate_limiter: &mut RateLimiter) -> BackendRouter {
  let router = BackendRouter::new()
    .nest("/password", password::router(rate_limiter))
    .nest("/logout", logout::router())
    .nest("/test_token", test_token::router());

  #[cfg(all(feature = "image", feature = "gravatar"))]
  {
    router
      .nest("/oidc", oidc::router(rate_limiter))
      .nest("/config", config::router())
  }
  #[cfg(not(all(feature = "image", feature = "gravatar")))]
  router
}

#[cfg(feature = "sea-orm")]
pub async fn state(router: BackendRouter, config: &AuthConfig, db: &Connection) -> BackendRouter {
  #[cfg(all(feature = "image", feature = "gravatar"))]
  use crate::backend::auth::oidc::OidcState;

  let pw_state = init_pw_state(config, db).await;
  let jwt_state = JwtState::init(config, db).await;
  #[cfg(all(feature = "image", feature = "gravatar"))]
  let oidc_state = OidcState::new(db).await;

  let router = router
    .layer(Extension(pw_state))
    .layer(Extension(jwt_state))
    .layer(Extension(JwtInvalidState::default()));

  #[cfg(all(feature = "image", feature = "gravatar"))]
  {
    router.layer(Extension(oidc_state))
  }
  #[cfg(not(all(feature = "image", feature = "gravatar")))]
  router
}

#[cfg(feature = "sea-orm")]
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
