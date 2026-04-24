use crate::{
  backend::{mail::state::ResetPasswordState, middleware::rate_limiter::RateLimiter},
  db::{init::Connection, tables::ConnectionExt},
  mail::{MailSettings, Mailer},
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

pub async fn state(router: ApiRouter, db: &Connection) -> ApiRouter {
  let settings: MailSettings = db.settings().get_settings().await.unwrap_or_default();
  let mailer = Mailer::new(settings).await;
  let password_reset_state = ResetPasswordState::default();

  router
    .layer(Extension(mailer))
    .layer(Extension(password_reset_state))
}
