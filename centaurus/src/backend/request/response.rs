use std::fmt::Debug;

use crate::anyhow;
use axum::{
  body::Body,
  response::{IntoResponse, Response},
};
use http::header::{CACHE_CONTROL, PRAGMA};
use serde::Serialize;
use tracing::instrument;

#[derive(Clone, Copy, Debug)]
pub struct TokenRes<T: Debug + Serialize = ()>(pub T);

impl<T: Debug + Serialize> IntoResponse for TokenRes<T> {
  #[instrument]
  fn into_response(self) -> Response {
    let Ok(body) = serde_json::to_string(&self.0) else {
      return anyhow!("Failed to serialize token response body").into_response();
    };

    Response::builder()
      .header(CACHE_CONTROL, "no-store")
      .header(PRAGMA, "no-cache")
      .body(Body::from(body))
      .unwrap()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use http::StatusCode;

  #[derive(Debug, Serialize)]
  struct Payload {
    id: u32,
  }

  #[tokio::test]
  async fn test_token_res_sets_no_cache_headers() {
    let response = TokenRes(Payload { id: 7 }).into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get(CACHE_CONTROL).unwrap(), "no-store");
    assert_eq!(response.headers().get(PRAGMA).unwrap(), "no-cache");

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
      .await
      .unwrap();
    assert_eq!(&body[..], br#"{"id":7}"#);
  }

  #[tokio::test]
  async fn test_token_res_unit_serializes_to_null() {
    let response = TokenRes(()).into_response();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
      .await
      .unwrap();
    assert_eq!(&body[..], b"null");
  }
}
