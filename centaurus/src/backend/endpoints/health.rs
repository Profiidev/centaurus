use axum::{Router, routing::get};

pub fn router() -> Router {
  Router::new().route("/health", get(health_check))
}

async fn health_check() -> &'static str {
  "OK"
}

#[cfg(test)]
mod tests {
  use super::*;
  use axum::body::Body;
  use http::{Request, StatusCode};
  use tower::ServiceExt;

  #[tokio::test]
  async fn test_health_check_returns_ok() {
    let app = router();
    let response = app
      .oneshot(
        Request::builder()
          .uri("/health")
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
      .await
      .unwrap();
    assert_eq!(&body[..], b"OK");
  }

  #[tokio::test]
  async fn test_unknown_route_is_not_found() {
    let response = router()
      .oneshot(
        Request::builder()
          .uri("/missing")
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
  }
}
