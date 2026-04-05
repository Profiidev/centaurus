use aide::axum::ApiRouter;
use aide::axum::routing::{ApiMethodRouter, post_with};
use axum_extra::extract::CookieJar;
use chrono::DateTime;
use http::StatusCode;
use tracing::debug;

use crate::backend::auth::jwt_auth::JwtAuth;
use crate::backend::auth::jwt_state::{JWT_COOKIE_NAME, JwtInvalidState, JwtState};
use crate::backend::res::TokenRes;
use crate::db::init::Connection;
use crate::db::tables::ConnectionExt;
use crate::error::{ErrorReportStatusExt, Result};

pub fn router() -> ApiRouter {
  ApiRouter::new().api_route("/", logout_route())
}

pub fn logout_route() -> ApiMethodRouter<()> {
  post_with(logout, |op| op.id("logout"))
}

async fn logout(
  auth: JwtAuth,
  db: Connection,
  mut cookies: CookieJar,
  state: JwtInvalidState,
  jwt: JwtState,
) -> Result<(CookieJar, TokenRes)> {
  let cookie = cookies
    .get(JWT_COOKIE_NAME)
    .status_context(StatusCode::UNAUTHORIZED, "Missing auth cookie")?;

  db.invalid_jwt()
    .invalidate_jwt(
      cookie.value().to_string(),
      DateTime::from_timestamp(auth.exp, 0)
        .status_context(StatusCode::INTERNAL_SERVER_ERROR, "invalid timestamp")?,
      state.count.clone(),
    )
    .await?;

  debug!("User logged out: {}", auth.user_id);
  cookies = cookies.remove(jwt.create_cookie(JWT_COOKIE_NAME, String::new()));

  Ok((cookies, TokenRes(())))
}
