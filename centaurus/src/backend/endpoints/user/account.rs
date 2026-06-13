use aide::axum::routing::{ApiMethodRouter, post_with};
#[cfg(feature = "avatar")]
use image::{ImageFormat, imageops::FilterType};
#[cfg(feature = "avatar")]
use std::io::Cursor;

use aide::axum::ApiRouter;
use axum::Json;
#[cfg(feature = "avatar")]
use base64::prelude::*;
use schemars::JsonSchema;
use serde::Deserialize;
use tower_governor::GovernorLayer;

#[cfg(feature = "avatar")]
use crate::error::ErrorReportStatusExt;
use crate::{
  backend::{
    auth::{jwt_auth::JwtAuth, pw_state::PasswordState},
    endpoints::{
      user::email::{confirm_email_change_route, start_email_change_route},
      websocket::state::{UpdateMessage, Updater},
    },
    middleware::rate_limiter::RateLimiter,
  },
  bail,
  db::{init::Connection, tables::ConnectionExt},
  error::Result,
};

pub fn router<T: UpdateMessage>(rate_limiter: &mut RateLimiter) -> ApiRouter {
  let router = ApiRouter::new()
    .api_route("/password", update_password_route())
    .api_route("/email_change_start", start_email_change_route())
    .layer(GovernorLayer::new(rate_limiter.create_limiter()))
    .api_route("/update", update_account_route::<T>())
    .api_route("/email_change_confirm", confirm_email_change_route::<T>());

  #[cfg(feature = "avatar")]
  {
    use axum::extract::DefaultBodyLimit;

    router.api_route(
      "/avatar",
      update_avatar_route::<T>().layer(DefaultBodyLimit::max(1024 * 1024 * 10)),
    )
  }
  #[cfg(not(feature = "avatar"))]
  {
    router
  }
}

#[cfg(feature = "avatar")]
pub fn update_avatar_route<T: UpdateMessage>() -> ApiMethodRouter<()> {
  post_with(update_avatar::<T>, |op| op.id("updateAvatar"))
}

pub fn update_password_route() -> ApiMethodRouter<()> {
  post_with(update_password, |op| op.id("updatePassword"))
}

pub fn update_account_route<T: UpdateMessage>() -> ApiMethodRouter<()> {
  post_with(update_account::<T>, |op| op.id("updateAccount"))
}

#[derive(Deserialize, JsonSchema)]
struct AccountUpdate {
  username: String,
}

async fn update_account<T: UpdateMessage>(
  auth: JwtAuth,
  db: Connection,
  updater: Updater<T>,
  Json(data): Json<AccountUpdate>,
) -> Result<()> {
  if data.username.trim().is_empty() {
    bail!(BAD_REQUEST, "Username cannot be empty");
  }

  db.user()
    .update_user_name(auth.user_id, data.username)
    .await?;
  updater.broadcast(T::user(auth.user_id)).await;
  Ok(())
}

#[cfg(feature = "avatar")]
#[derive(Deserialize, JsonSchema)]
struct AvatarUpdate {
  avatar: String,
}

#[cfg(feature = "avatar")]
async fn update_avatar<T: UpdateMessage>(
  auth: JwtAuth,
  db: Connection,
  updater: Updater<T>,
  Json(data): Json<AvatarUpdate>,
) -> Result<()> {
  if data.avatar.len() > 10 * 1024 * 1024 {
    bail!(PAYLOAD_TOO_LARGE, "Avatar size exceeds 10MB limit");
  }

  // Malformed client input (bad base64 or an undecodable image) is a 400, not
  // a 500 — consistent with the other base64 inputs in this crate.
  let raw_data = BASE64_STANDARD
    .decode(data.avatar)
    .status(http::StatusCode::BAD_REQUEST)?;
  let img = image::load_from_memory(&raw_data).status(http::StatusCode::BAD_REQUEST)?;
  let img = img.resize_exact(128, 128, FilterType::Lanczos3);

  let mut buf = Cursor::new(Vec::new());
  img.write_to(&mut buf, ImageFormat::WebP)?;

  db.user()
    .update_user_avatar(auth.user_id, buf.into_inner())
    .await?;
  updater.broadcast(T::user(auth.user_id)).await;
  Ok(())
}

#[derive(Deserialize, JsonSchema)]
struct PasswordUpdate {
  old_password: String,
  new_password: String,
}

async fn update_password(
  auth: JwtAuth,
  db: Connection,
  state: PasswordState,
  Json(data): Json<PasswordUpdate>,
) -> Result<()> {
  let user = db.user().get_user_by_id(auth.user_id).await?;

  if user.oidc_user {
    bail!(NOT_ACCEPTABLE, "OIDC users cannot change their password");
  }

  let old_hash = state.pw_hash(&user.salt, &data.old_password)?;
  if old_hash != user.password {
    bail!(FORBIDDEN, "Old password is incorrect");
  }

  let new_hash = state.pw_hash(&user.salt, &data.new_password)?;
  db.user()
    .update_user_password(auth.user_id, new_hash)
    .await?;
  Ok(())
}
