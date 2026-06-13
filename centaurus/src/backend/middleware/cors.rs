use axum::http::HeaderValue;
use http::{HeaderName, Method};
use tower_http::cors::CorsLayer;

use crate::{backend::config::BaseConfig, error::Result};

pub fn cors(config: &BaseConfig) -> Result<CorsLayer> {
  let cors = CorsLayer::new()
    .allow_methods([
      Method::GET,
      Method::POST,
      Method::PUT,
      Method::DELETE,
      Method::HEAD,
      Method::PATCH,
      Method::OPTIONS,
    ])
    .expose_headers([crate::VERSION_HEADER_NAME.parse::<HeaderName>()?])
    .allow_credentials(true);

  let mut origins = Vec::new();
  for origin in config.allowed_origins.split(',') {
    origins.push(origin.parse::<HeaderValue>()?);
  }

  Ok(cors.allow_origin(origins))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_cors_builds_with_default_config() {
    // Empty allowed_origins yields a single empty origin entry but still builds.
    let config = BaseConfig::default();
    assert!(cors(&config).is_ok());
  }

  #[test]
  fn test_cors_with_valid_origin() {
    let config = BaseConfig {
      allowed_origins: "https://example.com".into(),
      ..Default::default()
    };
    assert!(cors(&config).is_ok());
  }

  #[test]
  fn test_cors_rejects_invalid_origin() {
    // A newline is not a valid header value byte.
    let config = BaseConfig {
      allowed_origins: "https://ok.com,bad\norigin".into(),
      ..Default::default()
    };
    assert!(cors(&config).is_err());
  }
}
