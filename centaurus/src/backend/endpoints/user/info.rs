use aide::axum::ApiRouter;
use aide::axum::routing::{ApiMethodRouter, get_with};
use axum::Json;
#[cfg(feature = "avatar")]
use axum::extract::Path;
#[cfg(feature = "avatar")]
use http::StatusCode;
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
    router.api_route("/avatar/{uuid}", avatar_route())
  }

  #[cfg(not(feature = "avatar"))]
  router
}

pub fn info_route() -> ApiMethodRouter<()> {
  get_with(info, |op| op.id("info"))
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
  oidc_user: bool,
}

async fn info(auth: JwtAuth, db: Connection) -> Result<Json<UserInfo>> {
  let user = db.user().get_user_by_id(auth.user_id).await?;
  let permissions = db.group().get_user_permissions(auth.user_id).await?;

  Ok(Json(UserInfo {
    uuid: user.id,
    name: user.name,
    email: user.email,
    permissions,
    oidc_user: user.oidc_user,
  }))
}

#[cfg(feature = "avatar")]
#[derive(Deserialize, JsonSchema)]
struct AvatarPath {
  uuid: Uuid,
}

#[cfg(feature = "avatar")]
async fn avatar(
  _auth: JwtAuth,
  Path(path): Path<AvatarPath>,
  db: Connection,
) -> Result<std::result::Result<Vec<u8>, StatusCode>> {
  let Some(data) = db.user().get_user_avatar(path.uuid).await? else {
    return Ok(Err(StatusCode::NOT_FOUND));
  };
  Ok(Ok(data))
}
