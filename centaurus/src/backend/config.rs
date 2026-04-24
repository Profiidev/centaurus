#[cfg(feature = "config_site")]
use axum::{Extension, extract::FromRequestParts};
use serde::{Deserialize, Serialize};
use tracing::level_filters::LevelFilter;

use crate::serde::{de_str, se_str};

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BaseConfig {
  //base
  pub port: u16,

  #[serde(deserialize_with = "de_str", serialize_with = "se_str")]
  pub log_level: LevelFilter,

  pub allowed_origins: String,
}

impl Default for BaseConfig {
  fn default() -> Self {
    Self {
      port: 8000,
      log_level: LevelFilter::INFO,
      allowed_origins: "".to_string(),
    }
  }
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct MetricsConfig {
  pub metrics_enabled: bool,
  pub metrics_port: Option<u16>,
  pub metrics_name: String,
  pub extra_labels: Vec<(String, String)>,
}

pub trait Config: Clone + Send + Sync + 'static {
  fn base(&self) -> &BaseConfig;
  #[cfg(feature = "metrics")]
  fn metrics(&self) -> &MetricsConfig;
  #[cfg(feature = "config_site")]
  fn site(&self) -> &SiteConfig;
}

#[cfg(feature = "config_site")]
#[derive(Serialize, Deserialize, Debug, FromRequestParts, Clone)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema, aide::OperationIo))]
#[cfg_attr(feature = "db", derive(crate::Settings))]
#[cfg_attr(feature = "db", settings(id = 4))]
#[from_request(via(Extension))]
pub struct SiteConfig {
  pub site_url: url::Url,
}

#[cfg(feature = "config_site")]
impl Default for SiteConfig {
  fn default() -> Self {
    Self {
      site_url: url::Url::parse("http://localhost:8000").unwrap(),
    }
  }
}
