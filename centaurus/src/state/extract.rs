use axum::{Extension, RequestExt, RequestPartsExt, extract::Request};
use http::request::Parts;

#[async_trait::async_trait]
pub trait StateExtractExt {
  async fn extract_state<S: Clone + Sync + Send + 'static>(&mut self) -> S;
}

#[async_trait::async_trait]
impl StateExtractExt for Parts {
  async fn extract_state<S: Clone + Sync + Send + 'static>(&mut self) -> S {
    self
      .extract::<Extension<S>>()
      .await
      .unwrap_or_else(|_| {
        panic!(
          "Failed to extract state. Did you add the state with .layer(Extension({}))?",
          std::any::type_name::<S>()
        )
      })
      .0
  }
}

#[async_trait::async_trait]
impl StateExtractExt for Request {
  async fn extract_state<S: Clone + Sync + Send + 'static>(&mut self) -> S {
    self
      .extract_parts::<Extension<S>>()
      .await
      .unwrap_or_else(|_| {
        panic!(
          "Failed to extract state. Did you add the state with .layer(Extension({}))?",
          std::any::type_name::<S>()
        )
      })
      .0
  }
}
