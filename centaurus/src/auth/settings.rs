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
