use axum::http::HeaderValue;
use http::Method;
use tower_http::cors::CorsLayer;

use crate::config::BaseConfig;

#[cfg(feature = "error")]
type CorsResult = crate::error::Result<CorsLayer>;
#[cfg(not(feature = "error"))]
type CorsResult = CorsLayer;

pub fn cors(config: &BaseConfig) -> CorsResult {
  let cors = CorsLayer::new()
    .allow_methods([Method::GET, Method::POST])
    .allow_credentials(true);

  let mut origins = Vec::new();
  for origin in config.allowed_origins.split(',') {
    #[cfg(feature = "error")]
    origins.push(origin.parse::<HeaderValue>()?);
    #[cfg(not(feature = "error"))]
    origins.push(origin.parse::<HeaderValue>().expect("Invalid Header Value"));
  }

  #[cfg(feature = "error")]
  return Ok(cors.allow_origin(origins));
  #[cfg(not(feature = "error"))]
  cors.allow_origin(origins)
}
