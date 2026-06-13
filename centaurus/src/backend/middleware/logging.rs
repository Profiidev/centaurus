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

#[cfg(test)]
mod tests {
  use super::*;
  use axum::{body::Body, routing::get};
  use http::{Request, StatusCode};
  use tower::ServiceExt;

  fn finish(router: BackendRouter) -> axum::Router {
    #[cfg(feature = "openapi")]
    {
      router.finish_api(&mut aide::openapi::OpenApi::default())
    }
    #[cfg(not(feature = "openapi"))]
    {
      router
    }
  }

  #[tokio::test]
  async fn test_logging_layer_passes_requests_through() {
    // The logging layers must be transparent: the wrapped handler still runs
    // and its response is returned unchanged.
    let router = BackendRouter::new().route("/api/x", get(|| async { "ok" }));
    let app = finish(logging(router, |path| path.starts_with("/api")));

    let response = app
      .oneshot(Request::builder().uri("/api/x").body(Body::empty()).unwrap())
      .await
      .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
      .await
      .unwrap();
    assert_eq!(&body[..], b"ok");
  }

  #[tokio::test]
  async fn test_logging_layer_with_non_matching_filter() {
    // Exercise the branch where the filter returns false.
    let router = BackendRouter::new().route("/other", get(|| async { "y" }));
    let app = finish(logging(router, |path| path.starts_with("/api")));
    let response = app
      .oneshot(Request::builder().uri("/other").body(Body::empty()).unwrap())
      .await
      .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
  }
}
