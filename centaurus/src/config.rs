use serde::{Deserialize, Serialize};
#[cfg(feature = "logging")]
use tracing::level_filters::LevelFilter;

use crate::serde::{de_str, se_str};

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BaseConfig {
  //base
  pub port: u16,

  #[cfg(feature = "logging")]
  #[serde(deserialize_with = "de_str", serialize_with = "se_str")]
  pub log_level: LevelFilter,

  pub allowed_origins: String,
}

impl Default for BaseConfig {
  fn default() -> Self {
    Self {
      port: 8000,
      #[cfg(feature = "logging")]
      log_level: LevelFilter::INFO,
      allowed_origins: "".to_string(),
    }
  }
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct MetricsConfig {
  pub metrics_name: String,
  pub extra_labels: Vec<(String, String)>,
}
