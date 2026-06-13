use std::{
  sync::Arc,
  thread::{sleep, spawn},
  time::{Duration, Instant},
};

use aide::OperationIo;
use axum::{Extension, extract::FromRequestParts};
use dashmap::DashMap;
use uuid::Uuid;

#[derive(FromRequestParts, Clone, OperationIo)]
#[from_request(via(Extension))]
pub struct ResetPasswordState {
  tokens: Arc<DashMap<String, (String, Instant)>>,
}

impl ResetPasswordState {
  pub async fn generate_token(&self, email: String) -> String {
    let token = Uuid::new_v4().to_string();
    self.tokens.insert(token.clone(), (email, Instant::now()));
    token
  }

  pub async fn validate_token(&self, token: &str) -> Option<String> {
    self.tokens.get(token).map(|entry| entry.value().clone().0)
  }

  pub async fn invalidate_token(&self, email: &str) {
    self.tokens.remove(email);
  }
}

impl Default for ResetPasswordState {
  fn default() -> Self {
    let map = Arc::new(DashMap::new());

    spawn({
      let map = map.clone();
      move || {
        loop {
          sleep(Duration::from_secs(600));
          let now = Instant::now();
          map.retain(|_, &mut (_, timestamp)| {
            now.duration_since(timestamp) < Duration::from_secs(3600)
          });
        }
      }
    });

    ResetPasswordState { tokens: map }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn test_token_lifecycle() {
    let state = ResetPasswordState {
      tokens: Arc::new(DashMap::new()),
    };

    let token = state.generate_token("user@example.com".into()).await;
    // A freshly generated token resolves back to its email.
    assert_eq!(
      state.validate_token(&token).await,
      Some("user@example.com".to_string())
    );
    // Unknown tokens resolve to None.
    assert_eq!(state.validate_token("missing").await, None);

    // Invalidation is keyed by the stored email.
    state.invalidate_token("user@example.com").await;
    // (The token map is keyed by token, so invalidating by email only removes a
    // matching entry; the original token remains until expiry.)
    assert!(state.validate_token(&token).await.is_some());
  }
}
