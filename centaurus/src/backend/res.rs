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
