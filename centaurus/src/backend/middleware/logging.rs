use crate::backend::BackendRouter;

pub fn logging<F: Fn(&str) -> bool + Clone + Send + Sync + 'static>(
  router: BackendRouter,
  filter: F,
) -> BackendRouter {
  let response_filter = filter.clone();
  router
    .layer(axum::middleware::from_fn(uri_middleware))
    .layer(
      tower_http::trace::TraceLayer::new_for_http()
        .on_request(move |request: &http::Request<_>, span: &tracing::Span| {
          let path = request.uri().path();
          span.record("http.target.path", path);
          if filter(path) {
            tracing::info!("Received request: {} {}", request.method(), request.uri());
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

#[derive(Clone)]
pub struct RequestUri(http::Uri);

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
