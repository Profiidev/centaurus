use aide::axum::routing::post_with;
use image::{ImageFormat, imageops::FilterType};
use std::io::Cursor;

use aide::axum::ApiRouter;
use axum::Json;
use base64::prelude::*;
use schemars::JsonSchema;
use serde::Deserialize;
use tower_governor::GovernorLayer;

use crate::{
  backend::{
    auth::{jwt_auth::JwtAuth, pw_state::PasswordState},
    middleware::rate_limiter::RateLimiter,
    websocket::state::{UpdateMessage, Updater},
  },
  bail,
  db::{init::Connection, tables::ConnectionExt},
  error::Result,
};

pub fn router<T: UpdateMessage>(rate_limiter: &mut RateLimiter) -> ApiRouter {
  ApiRouter::new()
    .api_route(
      "/avatar",
      post_with(update_avatar::<T>, |op| op.id("updateAvatar")),
    )
    .api_route(
      "/password",
      post_with(update_password, |op| op.id("updatePassword")),
    )
    .layer(GovernorLayer::new(rate_limiter.create_limiter()))
    .api_route(
      "/update",
      post_with(update_account::<T>, |op| op.id("updateAccount")),
    )
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

#[derive(Deserialize, JsonSchema)]
struct AvatarUpdate {
  avatar: String,
}

async fn update_avatar<T: UpdateMessage>(
  auth: JwtAuth,
  db: Connection,
  updater: Updater<T>,
  Json(data): Json<AvatarUpdate>,
) -> Result<()> {
  if data.avatar.len() > 10 * 1024 * 1024 {
    bail!(PAYLOAD_TOO_LARGE, "Avatar size exceeds 10MB limit");
  }

  let raw_data = BASE64_STANDARD.decode(data.avatar)?;
  let img = image::load_from_memory(&raw_data)?;
  let img = img.resize_exact(128, 128, FilterType::Lanczos3);

  let mut buf = Cursor::new(Vec::new());
  img.write_to(&mut buf, ImageFormat::WebP)?;
  let avatar = BASE64_STANDARD.encode(buf.into_inner());

  db.user().update_user_avatar(auth.user_id, avatar).await?;
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
