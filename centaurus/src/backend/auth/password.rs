use axum::Json;
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use tower_governor::GovernorLayer;
use tracing::debug;
use uuid::Uuid;

use crate::backend::BackendRouter;
use crate::backend::auth::jwt_state::JwtState;
use crate::backend::auth::pw_state::PasswordState;
use crate::backend::middleware::rate_limiter::RateLimiter;
use crate::backend::res::TokenRes;
use crate::bail;
use crate::db::init::Connection;
use crate::db::tables::ConnectionExt;
use crate::error::Result;

pub fn router(rate_limiter: &mut RateLimiter) -> BackendRouter {
  #[cfg(feature = "openapi")]
  return BackendRouter::new()
    .api_route(
      "/",
      aide::axum::routing::post_with(authenticate, |op| op.id("authenticate")),
    )
    .layer(GovernorLayer::new(rate_limiter.create_limiter()))
    .api_route("/", aide::axum::routing::get_with(key, |op| op.id("key")));
  #[cfg(not(feature = "openapi"))]
  BackendRouter::new()
    .route("/", aide::axum::routing::post(authenticate))
    .layer(GovernorLayer::new(rate_limiter.create_limiter()))
    .route("/", aide::axum::routing::get(key))
}

#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema))]
struct KeyRes {
  key: String,
}

async fn key(state: PasswordState) -> Json<KeyRes> {
  Json(KeyRes { key: state.pub_key })
}

#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema))]
struct LoginReq {
  email: String,
  password: String,
}

#[derive(Serialize, Debug)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema))]
struct LoginResponse {
  user: Uuid,
}

async fn authenticate(
  state: PasswordState,
  jwt: JwtState,
  db: Connection,
  mut cookies: CookieJar,
  Json(req): Json<LoginReq>,
) -> Result<(CookieJar, TokenRes<LoginResponse>)> {
  let user = db.user().get_user_by_email(&req.email).await?;
  let hash = state.pw_hash(&user.salt, &req.password)?;

  if hash != user.password {
    bail!(UNAUTHORIZED, "Invalid email or password");
  }

  let cookie = jwt.create_token(user.id)?;
  cookies = cookies.add(cookie);
  debug!("User logged in: {}", user.id);

  Ok((cookies, TokenRes(LoginResponse { user: user.id })))
}
