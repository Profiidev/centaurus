use tracing::level_filters::LevelFilter;
use tracing_error::ErrorLayer;
use tracing_subscriber::{
  Layer, fmt::writer::MakeWriter, layer::SubscriberExt, util::SubscriberInitExt,
};

pub fn init_logging(log_level: LevelFilter) {
  init_logging_writer(log_level, std::io::stdout);
}

pub fn init_logging_stderr(log_level: LevelFilter) {
  init_logging_writer(log_level, std::io::stderr);
}

pub fn init_logging_writer<W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static>(
  log_level: LevelFilter,
  writer: W,
) {
  color_eyre::install().expect("Failed to install color_eyre");

  let layer = tracing_subscriber::fmt::layer()
    .with_writer(writer)
    .with_ansi(true)
    .with_filter(log_level);

  tracing_subscriber::registry()
    .with(layer)
    .with(ErrorLayer::default())
    .init();
}

#[cfg(test)]
mod tests {
  use super::*;

  // nextest runs each test in its own process, so installing the global
  // subscriber + color_eyre handler here does not clash with other tests.
  #[test]
  fn test_init_logging_installs_subscriber() {
    init_logging_writer(LevelFilter::DEBUG, std::io::sink);

    // Once installed, emitting events must not panic and a dispatcher is set.
    tracing::info!("hello from test");
    tracing::dispatcher::get_default(|d| {
      // A real (non no-op) subscriber is now the default.
      assert!(
        d.downcast_ref::<tracing::subscriber::NoSubscriber>()
          .is_none()
      );
    });
  }
}
