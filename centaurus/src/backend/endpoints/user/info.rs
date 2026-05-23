use aide::axum::ApiRouter;
use aide::axum::routing::{ApiMethodRouter, get_with};
use axum::Json;
#[cfg(feature = "avatar")]
use axum::extract::Path;
use schemars::JsonSchema;
#[cfg(feature = "avatar")]
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use crate::backend::auth::jwt_auth::JwtAuth;
use crate::db::init::Connection;
use crate::db::tables::ConnectionExt;
use crate::error::Result;

pub fn router() -> ApiRouter {
  let router = ApiRouter::new().api_route("/", info_route());

  #[cfg(feature = "avatar")]
  {
    router
      .api_route("/avatar", self_avatar_route())
      .api_route("/avatar/{uuid}", avatar_route())
  }

  #[cfg(not(feature = "avatar"))]
  router
}

pub fn info_route() -> ApiMethodRouter<()> {
  get_with(info, |op| op.id("info"))
}

#[cfg(feature = "avatar")]
pub fn self_avatar_route() -> ApiMethodRouter<()> {
  get_with(self_avatar, |op| op.id("avatar"))
}

#[cfg(feature = "avatar")]
pub fn avatar_route() -> ApiMethodRouter<()> {
  get_with(avatar, |op| op.id("avatarById"))
}

#[derive(Serialize, JsonSchema)]
struct UserInfo {
  uuid: Uuid,
  name: String,
  email: String,
  permissions: Vec<String>,
}

async fn info(auth: JwtAuth, db: Connection) -> Result<Json<UserInfo>> {
  let user = db.user().get_user_by_id(auth.user_id).await?;
  let permissions = db.group().get_user_permissions(auth.user_id).await?;

  Ok(Json(UserInfo {
    uuid: user.id,
    name: user.name,
    email: user.email,
    permissions,
  }))
}

#[cfg(feature = "avatar")]
async fn self_avatar(auth: JwtAuth, db: Connection) -> Result<Vec<u8>> {
  let Some(data) = db.user().get_user_avatar(auth.user_id).await? else {
    use crate::bail;
    bail!(NOT_FOUND, "Avatar not found");
  };
  Ok(data)
}

#[cfg(feature = "avatar")]
#[derive(Deserialize, JsonSchema)]
struct AvatarPath {
  uuid: Uuid,
}

#[cfg(feature = "avatar")]
async fn avatar(_auth: JwtAuth, Path(path): Path<AvatarPath>, db: Connection) -> Result<Vec<u8>> {
  let Some(data) = db.user().get_user_avatar(path.uuid).await? else {
    use crate::bail;
    bail!(NOT_FOUND, "Avatar not found");
  };
  Ok(data)
}
