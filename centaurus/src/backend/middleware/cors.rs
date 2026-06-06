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
