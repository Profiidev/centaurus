use axum::{Extension, extract::FromRequestParts};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "sea-orm", derive(crate::Settings))]
#[cfg_attr(feature = "sea-orm", settings(id = 2))]
pub struct UserSettings {
  pub oidc: Option<OidcSettings>,
  pub sso_instant_redirect: bool,
  pub sso_create_user: bool,
}

impl Default for UserSettings {
  fn default() -> Self {
    Self {
      oidc: None,
      sso_instant_redirect: true,
      sso_create_user: true,
    }
  }
}

#[derive(Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema))]
pub struct OidcSettings {
  pub issuer: Url,
  pub client_id: String,
  pub client_secret: String,
  pub scopes: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, FromRequestParts, Clone)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema, aide::OperationIo))]
#[cfg_attr(feature = "sea-orm", derive(crate::Settings))]
#[cfg_attr(feature = "sea-orm", settings(id = 4))]
#[from_request(via(Extension))]
pub struct SiteConfig {
  pub site_url: Url,
}

impl Default for SiteConfig {
  fn default() -> Self {
    Self {
      site_url: Url::parse("http://localhost:8000").unwrap(),
    }
  }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AuthConfig {
  pub auth_pepper: String,
  pub auth_issuer: String,
  pub auth_jwt_expiration: i64,
}

impl Default for AuthConfig {
  fn default() -> Self {
    Self {
      auth_issuer: "centaurus_auth".to_string(),
      auth_pepper: "__CENTAURUS_PEPPER__".to_string(),
      auth_jwt_expiration: 60 * 60 * 24 * 7, // 7 days
    }
  }
}
