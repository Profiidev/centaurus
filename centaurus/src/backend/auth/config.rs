use axum::Json;
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
  backend::{
    BackendRouter,
    auth::{oidc::OidcState, settings::UserSettings},
  },
  db::{init::Connection, tables::ConnectionExt},
  error::Result,
};

pub fn router() -> BackendRouter {
  #[cfg(feature = "openapi")]
  return BackendRouter::new().api_route(
    "/",
    aide::axum::routing::get_with(config, |op| op.id("authConfig")),
  );
  #[cfg(not(feature = "openapi"))]
  BackendRouter::new().route("/", axum::routing::get(config))
}

#[derive(Serialize, Debug, JsonSchema)]
enum SSOType {
  Oidc,
  None,
}

#[derive(Serialize, JsonSchema)]
struct AuthConfig {
  sso_type: SSOType,
  instant_redirect: bool,
  #[cfg(feature = "lettre")]
  mail_enabled: bool,
}

async fn config(
  oidc: OidcState,
  #[cfg(feature = "lettre")] mailer: crate::mail::Mailer,
  db: Connection,
) -> Result<Json<AuthConfig>> {
  let sso_type = if oidc.is_enabled().await {
    SSOType::Oidc
  } else {
    SSOType::None
  };

  let user_settings = db.settings().get_settings::<UserSettings>().await?;
  #[cfg(feature = "lettre")]
  let mail_enabled = mailer.is_active().await;

  Ok(Json(AuthConfig {
    sso_type,
    instant_redirect: user_settings.sso_instant_redirect,
    #[cfg(feature = "lettre")]
    mail_enabled,
  }))
}
