#[cfg(feature = "sea-orm")]
use axum::Extension;

#[cfg(feature = "sea-orm")]
use crate::{
  backend::{
    BackendRouter,
    auth::{
      jwt_state::{JwtInvalidState, JwtState},
      oidc::OidcState,
      pw_state::init_pw_state,
      settings::AuthConfig,
    },
    middleware::rate_limiter::RateLimiter,
  },
  db::init::Connection,
};

#[cfg(feature = "sea-orm")]
pub mod config;
pub mod jwt;
#[cfg(feature = "sea-orm")]
pub mod jwt_state;
#[cfg(feature = "sea-orm")]
pub mod jwt_auth;
#[cfg(feature = "sea-orm")]
pub mod logout;
#[cfg(feature = "sea-orm")]
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
  BackendRouter::new()
    .nest("/password", password::router(rate_limiter))
    .nest("/logout", logout::router())
    .nest("/test_token", test_token::router())
    .nest("/oidc", oidc::router(rate_limiter))
    .nest("/config", config::router())
}

#[cfg(feature = "sea-orm")]
pub async fn state(router: BackendRouter, config: &AuthConfig, db: &Connection) -> BackendRouter {
  let pw_state = init_pw_state(config, db).await;
  let jwt_state = JwtState::init(config, db).await;
  let oidc_state = OidcState::new(db).await;

  router
    .layer(Extension(pw_state))
    .layer(Extension(jwt_state))
    .layer(Extension(oidc_state))
    .layer(Extension(JwtInvalidState::default()))
}
