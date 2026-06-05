use std::convert::Infallible;

use axum::{
  Extension,
  extract::{FromRequestParts, OptionalFromRequestParts},
};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Serialize, Deserialize, Debug, FromRequestParts, Clone, Default)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema, aide::OperationIo))]
#[cfg_attr(feature = "db", derive(crate::Settings))]
#[cfg_attr(feature = "db", settings(id = 2))]
#[from_request(via(Extension))]
pub struct UserSettings {
  #[serde(default)]
  pub oidc_enabled: Option<bool>,
  pub oidc_issuer: Option<Url>,
  pub oidc_client_id: Option<String>,
  pub oidc_client_secret: Option<String>,
  pub oidc_scopes: Option<String>,
  pub oidc_group_sync: Option<bool>,
  pub oidc_group_claim: Option<String>,
  pub oidc_image_sync: Option<bool>,
  pub oidc_pkce: Option<bool>,
  pub sso_instant_redirect: Option<bool>,
  pub sso_create_user: Option<bool>,
}

impl<S: Send + Sync> OptionalFromRequestParts<S> for UserSettings {
  type Rejection = Infallible;

  async fn from_request_parts(
    parts: &mut http::request::Parts,
    state: &S,
  ) -> Result<Option<Self>, Self::Rejection> {
    Ok(
      <Self as FromRequestParts<S>>::from_request_parts(parts, state)
        .await
        .ok(),
    )
  }
}

impl UserSettings {
  pub fn oidc_settings(&self) -> Option<OidcSettings> {
    if self.oidc_enabled.unwrap_or(false)
      && let (Some(issuer), Some(client_id), Some(client_secret)) = (
        &self.oidc_issuer,
        &self.oidc_client_id,
        &self.oidc_client_secret,
      )
    {
      let scopes = self
        .oidc_scopes
        .clone()
        .map(|s| s.split(" ").map(|s| s.to_string()).collect())
        .unwrap_or_else(|| vec!["openid".to_string()]);
      Some(OidcSettings {
        issuer: issuer.clone(),
        client_id: client_id.clone(),
        client_secret: client_secret.clone(),
        image_sync: self.oidc_image_sync.unwrap_or(false),
        group_sync: self.oidc_group_sync.unwrap_or(false),
        group_claim: self
          .oidc_group_claim
          .clone()
          .unwrap_or_else(|| "groups".to_string()),
        scopes,
        create_user: self.sso_create_user.unwrap_or(false),
        pkce: self.oidc_pkce.unwrap_or(false),
      })
    } else {
      None
    }
  }
}

#[derive(Debug, Clone)]
pub struct OidcSettings {
  pub issuer: Url,
  pub client_id: String,
  pub client_secret: String,
  pub scopes: Vec<String>,
  pub group_sync: bool,
  pub group_claim: String,
  pub pkce: bool,
  pub image_sync: bool,
  pub create_user: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
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
