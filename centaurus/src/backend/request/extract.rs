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

#[cfg(test)]
mod tests {
  use super::*;
  use axum::body::Body;

  #[derive(Clone, Debug, PartialEq)]
  struct MyState(u32);

  #[tokio::test]
  async fn test_extract_present_state() {
    let mut parts = Request::builder()
      .extension(MyState(7))
      .body(Body::empty())
      .unwrap()
      .into_parts()
      .0;
    let state: MyState = parts.extract_state().await;
    assert_eq!(state, MyState(7));
  }

  #[tokio::test]
  #[should_panic(expected = "Failed to extract state")]
  async fn test_extract_missing_state_panics() {
    let mut parts = Request::builder()
      .body(Body::empty())
      .unwrap()
      .into_parts()
      .0;
    let _: MyState = parts.extract_state().await;
  }
}
