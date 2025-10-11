use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};
#[cfg(feature = "logging")]
use tracing::level_filters::LevelFilter;

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

pub fn de_str<'de, D: Deserializer<'de>, T: FromStr>(d: D) -> Result<T, D::Error>
where
  T::Err: std::fmt::Display,
{
  let s = String::deserialize(d)?;
  T::from_str(&s).map_err(serde::de::Error::custom)
}

pub fn se_str<T: ToString, S: serde::Serializer>(t: &T, s: S) -> Result<S::Ok, S::Error> {
  s.serialize_str(&t.to_string())
}
