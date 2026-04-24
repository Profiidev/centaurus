use aide::axum::ApiRouter;
use aide::axum::routing::{ApiMethodRouter, delete_with, get_with, post_with, put_with};
use argon2::password_hash::SaltString;
use axum::{Json, extract::Path};
use base64::prelude::*;
use http::StatusCode;
use rand::RngExt;
use rsa::rand_core::OsRng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::backend::auth::jwt_auth::JwtAuth;
use crate::backend::auth::permission::{UserEdit, UserView};
use crate::backend::auth::pw_state::PasswordState;
use crate::backend::config::SiteConfig;
use crate::backend::endpoints::user::template;
use crate::backend::endpoints::websocket::state::{UpdateMessage, Updater};
use crate::bail;
use crate::db::init::Connection;
use crate::db::tables::ConnectionExt;
use crate::db::tables::user::{DetailUserInfo, SimpleGroupInfo, UserListInfo};
use crate::error::{ErrorReportStatusExt, Result};
use crate::mail::Mailer;

const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789)(*&^%$#@!~";

pub fn router<T: UpdateMessage>() -> ApiRouter {
  #[cfg(feature = "avatar")]
  let router = ApiRouter::new().api_route("/avatar", reset_user_avatar_route::<T>());
  #[cfg(not(feature = "avatar"))]
  let router = ApiRouter::new();

  router
    .api_route("/", list_users_route())
    .api_route("/", create_user_route::<T>())
    .api_route("/", delete_user_route::<T>())
    .api_route("/", edit_user_route::<T>())
    .api_route("/{uuid}", user_info_route())
    .api_route("/mail", mail_active_route())
    .api_route("/groups", list_groups_simple_route())
    .api_route("/password", reset_user_password_route())
}

pub fn list_users_route() -> ApiMethodRouter<()> {
  get_with(list_users, |op| op.id("listUsers"))
}

pub fn create_user_route<T: UpdateMessage>() -> ApiMethodRouter<()> {
  post_with(create_user::<T>, |op| op.id("createUser"))
}

pub fn delete_user_route<T: UpdateMessage>() -> ApiMethodRouter<()> {
  delete_with(delete_user::<T>, |op| op.id("deleteUser"))
}

pub fn edit_user_route<T: UpdateMessage>() -> ApiMethodRouter<()> {
  put_with(edit_user::<T>, |op| op.id("editUser"))
}

pub fn user_info_route() -> ApiMethodRouter<()> {
  get_with(user_info, |op| op.id("userInfo"))
}

pub fn mail_active_route() -> ApiMethodRouter<()> {
  get_with(mail_active, |op| op.id("mailActive"))
}

pub fn list_groups_simple_route() -> ApiMethodRouter<()> {
  get_with(list_groups_simple, |op| op.id("listGroupsSimple"))
}

#[cfg(feature = "avatar")]
pub fn reset_user_avatar_route<T: UpdateMessage>() -> ApiMethodRouter<()> {
  delete_with(reset_user_avatar::<T>, |op| op.id("resetUserAvatar"))
}

pub fn reset_user_password_route() -> ApiMethodRouter<()> {
  put_with(reset_user_password, |op| op.id("resetUserPassword"))
}

async fn list_users(_auth: JwtAuth<UserView>, db: Connection) -> Result<Json<Vec<UserListInfo>>> {
  let users = db.user().list_users().await?;
  Ok(Json(users))
}

#[derive(Deserialize, JsonSchema)]
struct UserViewPath {
  uuid: Uuid,
}

async fn user_info(
  _auth: JwtAuth<UserView>,
  db: Connection,
  Path(path): Path<UserViewPath>,
) -> Result<Json<DetailUserInfo>> {
  let info = db.user().user_info(path.uuid).await?;
  let Some(info) = info else {
    bail!(NOT_FOUND, "User not found");
  };
  Ok(Json(info))
}

#[derive(Serialize, JsonSchema)]
struct MailActiveResponse {
  active: bool,
}

async fn mail_active(_auth: JwtAuth<UserView>, mailer: Mailer) -> Result<Json<MailActiveResponse>> {
  let active = mailer.is_active().await;
  Ok(Json(MailActiveResponse { active }))
}

#[derive(Deserialize, JsonSchema)]
struct CreateUser {
  name: String,
  email: String,
  password: Option<String>,
}

#[derive(Serialize, JsonSchema)]
struct CreateUserResponse {
  uuid: Uuid,
}

async fn create_user<T: UpdateMessage>(
  _auth: JwtAuth<UserEdit>,
  db: Connection,
  updater: Updater<T>,
  mailer: Mailer,
  state: PasswordState,
  config: SiteConfig,
  Json(req): Json<CreateUser>,
) -> Result<Json<CreateUserResponse>> {
  if req.name.trim().is_empty() {
    bail!(BAD_REQUEST, "Name cannot be empty");
  }

  if req.email.trim().is_empty() {
    bail!(BAD_REQUEST, "Email cannot be empty");
  }

  if db.user().try_get_user_by_email(&req.email).await?.is_some() {
    bail!(CONFLICT, "User with this email already exists");
  }

  let password = if mailer.is_active().await {
    let mut rng = rand::rng();
    (0..12)
      .map(|_| {
        let idx = rng.random_range(0..CHARSET.len());
        CHARSET[idx] as char
      })
      .collect::<String>()
  } else if let Some(pw) = req.password {
    let bytes = BASE64_STANDARD.decode(pw).status(StatusCode::BAD_REQUEST)?;
    let pw_bytes = state.decrypt(&bytes).status(StatusCode::BAD_REQUEST)?;
    String::from_utf8_lossy(&pw_bytes).to_string()
  } else {
    bail!(
      BAD_REQUEST,
      "Password must be provided when mail service is not active"
    );
  };

  let salt = SaltString::generate(OsRng {}).to_string();
  let password_hash = state.pw_hash_raw(&salt, &password)?;

  let user_id = db
    .user()
    .create_user(req.name.clone(), req.email.clone(), password_hash, salt)
    .await?;
  if mailer.is_active().await {
    let subject = "Your new account";
    mailer
      .send_mail(
        req.name,
        req.email,
        subject.to_string(),
        template::init_password(config.site_url.as_str(), &password),
      )
      .await?;
  }
  updater.broadcast(T::user(user_id)).await;

  Ok(Json(CreateUserResponse { uuid: user_id }))
}

#[derive(Deserialize, JsonSchema)]
struct DeleteUserRequest {
  uuid: Uuid,
}

async fn delete_user<T: UpdateMessage>(
  auth: JwtAuth<UserEdit>,
  db: Connection,
  updater: Updater<T>,
  Json(data): Json<DeleteUserRequest>,
) -> Result<()> {
  let Some(admin_group) = db.setup().get_admin_group_id().await? else {
    bail!(INTERNAL_SERVER_ERROR, "Admin group is not set up");
  };

  if db.group().is_last_admin(admin_group, data.uuid).await? {
    bail!(CONFLICT, "Cannot delete the last user from the admin group");
  }

  if db.group().is_in_group(admin_group, data.uuid).await?
    && !db.group().is_in_group(admin_group, auth.user_id).await?
  {
    bail!(
      FORBIDDEN,
      "User cannot delete another user with higher permissions"
    );
  }

  db.user().delete_user(data.uuid).await?;
  updater.broadcast(T::user(data.uuid)).await;

  Ok(())
}

async fn list_groups_simple(
  _auth: JwtAuth<UserView>,
  db: Connection,
) -> Result<Json<Vec<SimpleGroupInfo>>> {
  let groups = db.group().list_groups_simple().await?;
  Ok(Json(groups))
}

#[derive(Deserialize, JsonSchema)]
struct UserEditReq {
  uuid: Uuid,
  name: String,
  groups: Vec<Uuid>,
}

async fn edit_user<T: UpdateMessage>(
  auth: JwtAuth<UserEdit>,
  db: Connection,
  updater: Updater<T>,
  Json(req): Json<UserEditReq>,
) -> Result<()> {
  if req.name.trim().is_empty() {
    bail!(BAD_REQUEST, "Name cannot be empty");
  }

  let self_permissions = db.group().get_user_permissions(auth.user_id).await?;
  let target_permissions = db
    .group()
    .get_groups_permissions(req.groups.clone())
    .await?;
  let current_user_permissions = db.group().get_user_permissions(req.uuid).await?;

  if target_permissions
    .iter()
    .any(|p| !self_permissions.contains(p))
    || current_user_permissions
      .iter()
      .any(|p| !self_permissions.contains(p))
  {
    bail!(
      FORBIDDEN,
      "Cannot assign permissions that the editor does not have"
    );
  }

  let Some(admin_group) = db.setup().get_admin_group_id().await? else {
    bail!(INTERNAL_SERVER_ERROR, "Admin group is not set up");
  };

  if !req.groups.contains(&admin_group) && db.group().is_last_admin(admin_group, req.uuid).await? {
    bail!(CONFLICT, "Cannot remove the last user from the admin group");
  }

  let Some(_user) = db.user().user_info(req.uuid).await? else {
    bail!(NOT_FOUND, "User not found");
  };

  db.user().edit_user(req.uuid, req.name, req.groups).await?;
  updater.broadcast(T::user(req.uuid)).await;

  Ok(())
}

#[derive(Deserialize, JsonSchema)]
struct UserAvatarResetRequest {
  uuid: Uuid,
}

#[cfg(feature = "avatar")]
async fn reset_user_avatar<T: UpdateMessage>(
  _auth: JwtAuth<UserEdit>,
  db: Connection,
  updater: Updater<T>,
  Json(req): Json<UserAvatarResetRequest>,
) -> Result<()> {
  db.user().reset_avatar(req.uuid).await?;
  updater.broadcast(T::user(req.uuid)).await;

  Ok(())
}

#[derive(Deserialize, JsonSchema)]
struct ResetUserPassword {
  uuid: Uuid,
  new_password: String,
}

async fn reset_user_password(
  auth: JwtAuth<UserEdit>,
  db: Connection,
  state: PasswordState,
  mailer: Mailer,
  Json(req): Json<ResetUserPassword>,
) -> Result<()> {
  if mailer.is_active().await {
    bail!(
      BAD_REQUEST,
      "Cannot reset password when mail service is active"
    );
  }

  let self_permissions = db.group().get_user_permissions(auth.user_id).await?;
  let target_permissions = db.group().get_user_permissions(req.uuid).await?;

  if target_permissions
    .iter()
    .any(|p| !self_permissions.contains(p))
  {
    bail!(
      FORBIDDEN,
      "Cannot reset password for a user with higher permissions"
    );
  }

  let user = db.user().get_user_by_id(req.uuid).await?;
  let hash = state.pw_hash(&user.salt, &req.new_password)?;
  db.user().update_user_password(req.uuid, hash).await?;

  Ok(())
}
