use crate::{
  backend::{
    auth::pw_state::PasswordState,
    config::SiteConfig,
    mail::{state::ResetPasswordState, template},
  },
  db::{init::Connection, tables::ConnectionExt},
  mail::Mailer,
};
use aide::axum::routing::post_with;
use aide::axum::{ApiRouter, routing::ApiMethodRouter};
use axum::Json;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::spawn;
use tracing::warn;

pub fn router() -> ApiRouter {
  ApiRouter::new()
    .api_route("/send", send_reset_link_route())
    .api_route("/confirm", reset_password_route())
}

pub fn send_reset_link_route() -> ApiMethodRouter<()> {
  post_with(send_reset_link, |op| op.id("sendResetLink"))
}

pub fn reset_password_route() -> ApiMethodRouter<()> {
  post_with(reset_password, |op| op.id("resetPassword"))
}

#[derive(JsonSchema, Deserialize)]
struct ResetRequest {
  email: String,
}

#[axum::debug_handler]
async fn send_reset_link(
  mailer: Mailer,
  state: ResetPasswordState,
  db: Connection,
  config: SiteConfig,
  Json(ResetRequest { email }): Json<ResetRequest>,
) -> Result<(), ()> {
  // Spawn a new task to handle the email sending asynchronously and avoid exposing timing information
  spawn(async move {
    let Some(user) = db.user().get_user_by_email(&email).await.ok() else {
      warn!("Password reset requested for non-existent email: {}", email);
      return;
    };

    let token = state.generate_token(user.email.clone()).await;

    let mut reset_link = config.site_url.clone();
    if let Ok(segments) = &mut reset_link.path_segments_mut() {
      segments.pop_if_empty();
      segments.push("password");
      segments.push("reset");
    }
    reset_link.query_pairs_mut().append_pair("token", &token);

    if let Err(e) = mailer
      .send_mail(
        user.name,
        user.email,
        "Hibernation Password Reset".to_string(),
        template::reset_link(reset_link.as_str(), config.site_url.as_str()),
      )
      .await
    {
      warn!("Failed to send password reset email to {}: {:?}", email, e);
    }
  });

  Ok(())
}

#[derive(JsonSchema, Deserialize)]
struct ResetPasswordPayload {
  token: String,
  new_password: String,
}

async fn reset_password(
  db: Connection,
  state: ResetPasswordState,
  pw: PasswordState,
  Json(ResetPasswordPayload {
    token,
    new_password,
  }): Json<ResetPasswordPayload>,
) -> Result<(), ()> {
  // Do reset async to avoid exposing timing information
  spawn(async move {
    let Some(email) = state.validate_token(&token).await else {
      warn!("Invalid or expired password reset token used");
      return;
    };

    let Some(user) = db.user().get_user_by_email(&email).await.ok() else {
      warn!("Password reset attempted for non-existent email: {}", email);
      return;
    };

    let Ok(hashed_password) = pw.pw_hash(&user.salt, &new_password) else {
      warn!("Failed to hash new password for {}", email);
      return;
    };

    if let Err(e) = db
      .user()
      .update_user_password(user.id, hashed_password)
      .await
    {
      warn!("Failed to update password for {}: {:?}", email, e);
    }

    state.invalidate_token(&email).await;
  });

  Ok(())
}
