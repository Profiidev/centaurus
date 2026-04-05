use aide::axum::ApiRouter;
use aide::axum::routing::{ApiMethodRouter, get_with};
use axum::Json;
use schemars::JsonSchema;
use serde::Serialize;
use uuid::Uuid;

use crate::backend::auth::jwt_auth::JwtAuth;
use crate::db::init::Connection;
use crate::db::tables::ConnectionExt;
use crate::error::Result;

pub fn router() -> ApiRouter {
  ApiRouter::new().api_route("/", info_route())
}

pub fn info_route() -> ApiMethodRouter<()> {
  get_with(info, |op| op.id("info"))
}

#[derive(Serialize, JsonSchema)]
struct UserInfo {
  uuid: Uuid,
  name: String,
  email: String,
  permissions: Vec<String>,
  avatar: Option<String>,
}

async fn info(auth: JwtAuth, db: Connection) -> Result<Json<UserInfo>> {
  let user = db.user().get_user_by_id(auth.user_id).await?;
  let permissions = db.group().get_user_permissions(auth.user_id).await?;

  Ok(Json(UserInfo {
    uuid: user.id,
    name: user.name,
    email: user.email,
    permissions,
    avatar: user.avatar,
  }))
}
