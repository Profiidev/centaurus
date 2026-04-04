use tracing::level_filters::LevelFilter;
use tracing_error::ErrorLayer;
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_logging(log_level: LevelFilter) {
  color_eyre::install().expect("Failed to install color_eyre");

  let layer = tracing_subscriber::fmt::layer()
    .with_ansi(true)
    .with_filter(log_level);

  tracing_subscriber::registry()
    .with(layer)
    .with(ErrorLayer::default())
    .init();
}
