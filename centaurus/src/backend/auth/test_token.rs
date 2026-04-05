use axum::Json;
use axum_extra::extract::CookieJar;
use serde::Serialize;

use crate::backend::{
  BackendRouter,
  auth::{
    jwt_state::{JWT_COOKIE_NAME, JwtState},
    jwt_auth::JwtAuth,
  },
};

pub fn router() -> BackendRouter {
  #[cfg(feature = "openapi")]
  return BackendRouter::new().api_route(
    "/",
    aide::axum::routing::get_with(test_token, |op| op.id("testToken")),
  );
  #[cfg(not(feature = "openapi"))]
  BackendRouter::new().route("/", aide::axum::routing::get(test_token))
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
