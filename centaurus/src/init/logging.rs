use tracing_error::ErrorLayer;
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::BaseConfig;

pub fn init_logging(config: &BaseConfig) {
  color_eyre::install().expect("Failed to install color_eyre");

  let layer = tracing_subscriber::fmt::layer()
    .with_ansi(true)
    .with_filter(config.log_level);

  tracing_subscriber::registry()
    .with(layer)
    .with(ErrorLayer::default())
    .init();
}

#[cfg(all(feature = "logging", feature = "axum"))]
#[allow(non_camel_case_types, async_fn_in_trait)]
pub trait logging {
  fn logging<F: Fn(&str) -> bool + Clone + Send + Sync + 'static>(self, filter: F) -> Self;
}

impl logging for axum::Router {
  fn logging<F: Fn(&str) -> bool + Clone + Send + Sync + 'static>(self, filter: F) -> Self {
    self.layer(
      tower_http::trace::TraceLayer::new_for_http()
        .on_request(move |request: &http::Request<_>, span: &tracing::Span| {
          let path = request.uri().path();
          span.record("http.target.path", path);
          if filter(path) {
            tracing::info!("Received request: {}", request.uri());
          }
        })
        .on_response(
          |response: &http::Response<_>, latency: std::time::Duration, span: &tracing::Span| {
            let path = span.metadata().unwrap().fields();
            dbg!(path);
            tracing::info!(
              "Response sent with status: {} in {:?}",
              response.status(),
              latency
            );
          },
        ),
    )
  }
}
