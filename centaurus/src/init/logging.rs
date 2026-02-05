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

#[cfg(all(feature = "logging", feature = "axum"))]
#[allow(non_camel_case_types, async_fn_in_trait)]
pub trait logging {
  fn logging<F: Fn(&str) -> bool + Clone + Send + Sync + 'static>(self, filter: F) -> Self;
}

#[cfg(all(feature = "logging", feature = "axum"))]
impl logging for axum::Router {
  fn logging<F: Fn(&str) -> bool + Clone + Send + Sync + 'static>(self, filter: F) -> Self {
    let response_filter = filter.clone();
    self.layer(axum::middleware::from_fn(uri_middleware)).layer(
      tower_http::trace::TraceLayer::new_for_http()
        .on_request(move |request: &http::Request<_>, span: &tracing::Span| {
          let path = request.uri().path();
          span.record("http.target.path", path);
          if filter(path) {
            tracing::info!("Received request: {}", request.uri());
          }
        })
        .on_response(
          move |response: &http::Response<_>,
                latency: std::time::Duration,
                _span: &tracing::Span| {
            let uri = response.extensions().get::<RequestUri>().unwrap();
            let path = uri.0.path();
            if response_filter(path) {
              tracing::info!(
                "Response sent with status: {} {} in {:?}",
                response.status(),
                uri.0,
                latency
              );
            }
          },
        ),
    )
  }
}

#[derive(Clone)]
#[cfg(all(feature = "logging", feature = "axum"))]
pub struct RequestUri(http::Uri);

#[cfg(all(feature = "logging", feature = "axum"))]
async fn uri_middleware(
  req: axum::extract::Request,
  next: axum::middleware::Next,
) -> axum::response::Response {
  let path = req.uri().clone();
  let uri = RequestUri(path);

  let mut response = next.run(req).await;
  response.extensions_mut().insert(uri);
  response
}
