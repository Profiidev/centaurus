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
crate::router_extension!(
  async fn logging(self) -> Self {
    self.layer(
      tower_http::trace::TraceLayer::new_for_http()
        .on_request(|request: &http::Request<_>, _span: &tracing::Span| {
          tracing::info!("Received request: {}", request.uri());
        })
        .on_response(
          |response: &http::Response<_>, latency: std::time::Duration, _span: &tracing::Span| {
            tracing::info!(
              "Response sent with status: {} in {:?}",
              response.status(),
              latency
            );
          },
        ),
    )
  }
);
