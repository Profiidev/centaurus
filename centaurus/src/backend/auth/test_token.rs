use aide::axum::routing::{ApiMethodRouter, get_with};
use axum::Json;
use axum_extra::extract::CookieJar;
use serde::Serialize;

use crate::backend::{
  BackendRouter,
  auth::{
    jwt_auth::JwtAuth,
    jwt_state::{JWT_COOKIE_NAME, JwtState},
  },
};

pub fn router() -> BackendRouter {
  BackendRouter::new().api_route("/", test_token_route())
}

pub fn test_token_route() -> ApiMethodRouter<()> {
  get_with(test_token, |op| op.id("testToken"))
}

#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema))]
struct TestTokenResponse {
  valid: bool,
}

async fn test_token(
  auth: Option<JwtAuth>,
  mut cookies: CookieJar,
  jwt: JwtState,
) -> (CookieJar, Json<TestTokenResponse>) {
  if auth.is_none() {
    cookies = cookies.remove(jwt.create_cookie(JWT_COOKIE_NAME, String::new()));

    (cookies, Json(TestTokenResponse { valid: false }))
  } else {
    (cookies, Json(TestTokenResponse { valid: true }))
  }
}
