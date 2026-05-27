use std::convert::Infallible;

use axum::{
  Extension,
  extract::{FromRequestParts, OptionalFromRequestParts},
};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Serialize, Deserialize, Debug, FromRequestParts, Clone)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema, aide::OperationIo))]
#[cfg_attr(feature = "db", derive(crate::Settings))]
#[cfg_attr(feature = "db", settings(id = 2))]
#[from_request(via(Extension))]
pub struct UserSettings {
  #[serde(default)]
  pub oidc_enabled: bool,
  pub oidc_issuer: Option<Url>,
  pub oidc_client_id: Option<String>,
  pub oidc_client_secret: Option<String>,
  pub oidc_scopes: Option<String>,
  #[serde(default)]
  pub oidc_group_sync: bool,
  #[serde(default)]
  pub oidc_group_claim: Option<String>,
  #[serde(default)]
  pub oidc_image_sync: bool,
  pub sso_instant_redirect: bool,
  pub sso_create_user: bool,
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

impl Default for UserSettings {
  fn default() -> Self {
    Self {
      sso_instant_redirect: true,
      sso_create_user: true,
      oidc_enabled: false,
      oidc_issuer: None,
      oidc_client_id: None,
      oidc_client_secret: None,
      oidc_scopes: None,
      oidc_group_claim: None,
      oidc_group_sync: false,
      oidc_image_sync: false,
    }
  }
}

impl UserSettings {
  pub fn oidc_settings(&self) -> Option<OidcSettings> {
    if self.oidc_enabled
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
        image_sync: self.oidc_image_sync,
        group_sync: self.oidc_group_sync,
        group_claim: self
          .oidc_group_claim
          .clone()
          .unwrap_or_else(|| "groups".to_string()),
        scopes,
      })
    } else {
      None
    }
  }
}

pub struct OidcSettings {
  pub issuer: Url,
  pub client_id: String,
  pub client_secret: String,
  pub scopes: Vec<String>,
  pub group_sync: bool,
  pub group_claim: String,
  pub image_sync: bool,
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
