use crate::{
  backend::{
    config::Config, endpoints::mail::state::ResetPasswordState,
    middleware::rate_limiter::RateLimiter,
  },
  db::{init::Connection, tables::ConnectionExt},
  mail::{MailSettings, Mailer},
  overwrite_with_env_config,
};
use aide::axum::ApiRouter;
use axum::Extension;
use tower_governor::GovernorLayer;

mod reset;
pub mod state;
pub mod template;
mod test;

pub fn router(rate_limiter: &mut RateLimiter) -> ApiRouter {
  ApiRouter::new()
    .nest("/reset", reset::router())
    .nest("/test", test::router())
    .layer(GovernorLayer::new(rate_limiter.create_limiter()))
}

pub async fn state<C: Config>(router: ApiRouter, db: &Connection, config: &C) -> ApiRouter {
  let mut settings: MailSettings = db.settings().get_settings().await.unwrap_or_default();
  let mail = config.mail();

  overwrite_with_env_config!(
    settings,
    mail,
    smtp_server,
    smtp_port,
    smtp_username,
    smtp_password,
    smtp_from_address,
    smtp_from_name,
    smtp_use_tls,,
    smtp_enabled
  );

  let mailer = Mailer::new(settings).await;
  let password_reset_state = ResetPasswordState::default();

  router
    .layer(Extension(mailer))
    .layer(Extension(password_reset_state))
}
