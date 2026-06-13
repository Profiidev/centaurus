use std::{fmt::Debug, sync::Arc};

use axum::{
  Extension, RequestPartsExt,
  extract::{FromRequestParts, rejection::ExtensionRejection},
};
use dashmap::DashMap;
use serde::{Serialize, de::DeserializeOwned};
use tokio::{
  spawn,
  sync::mpsc::{self, Receiver, Sender},
  task::JoinHandle,
};
use tracing::debug;
use uuid::Uuid;

pub trait UpdateMessage: Serialize + DeserializeOwned + Clone + Debug + Send + 'static {
  fn settings() -> Self;
  fn group(uuid: Uuid) -> Self;
  fn user(uuid: Uuid) -> Self;
  fn user_permissions() -> Self;
}

#[derive(Clone)]
#[cfg_attr(feature = "openapi", derive(aide::OperationIo))]
pub struct UpdateState<T: UpdateMessage> {
  sessions: Arc<DashMap<Uuid, DashMap<Uuid, Sender<T>>>>,
  #[allow(dead_code)]
  update_proxy: Arc<JoinHandle<()>>,
}

impl<T: UpdateMessage, R: Sync> FromRequestParts<R> for UpdateState<T> {
  type Rejection = ExtensionRejection;

  async fn from_request_parts(
    parts: &mut http::request::Parts,
    _state: &R,
  ) -> Result<Self, Self::Rejection> {
    parts
      .extract::<Extension<UpdateState<T>>>()
      .await
      .map(|ex| ex.0)
  }
}

#[derive(Clone)]
#[cfg_attr(feature = "openapi", derive(aide::OperationIo))]
pub struct Updater<T: UpdateMessage>(Sender<UpdateTrigger<T>>);

impl<T: UpdateMessage, R: Sync> FromRequestParts<R> for Updater<T> {
  type Rejection = ExtensionRejection;

  async fn from_request_parts(
    parts: &mut http::request::Parts,
    _state: &R,
  ) -> Result<Self, Self::Rejection> {
    parts
      .extract::<Extension<Updater<T>>>()
      .await
      .map(|ex| ex.0)
  }
}

pub struct UpdateTrigger<T: DeserializeOwned + Serialize + Clone + Debug + Send + 'static> {
  target: Option<Uuid>,
  message: T,
}

impl<T: UpdateMessage> UpdateState<T> {
  pub async fn init() -> (Self, Updater<T>) {
    let sessions: Arc<DashMap<Uuid, DashMap<Uuid, Sender<T>>>> = Arc::new(DashMap::default());
    let (sender, mut receiver) = mpsc::channel(100);
    let updater: Updater<T> = Updater(sender);

    let update_proxy = spawn({
      let sessions = sessions.clone();
      async move {
        while let Some(message) = receiver.recv().await {
          if let Some(target) = message.target {
            debug!(
              "Sending update message to {}: {:?}",
              target, message.message
            );
            if let Some(pair) = sessions.get(&target) {
              for pair in pair.value().iter() {
                pair.value().send(message.message.clone()).await.ok();
              }
            }
          } else {
            debug!("Broadcasting update message: {:?}", message.message);
            for pair in sessions.iter() {
              for pair in pair.value().iter() {
                pair.value().send(message.message.clone()).await.ok();
              }
            }
          }
        }
      }
    });

    let state = Self {
      sessions,
      update_proxy: Arc::new(update_proxy),
    };

    (state, updater)
  }

  pub async fn create_session(&self, user: Uuid) -> (Uuid, Receiver<T>) {
    let (send, recv) = mpsc::channel(100);
    let user_sessions = self.sessions.entry(user).or_default();
    let uuid = Uuid::new_v4();
    user_sessions.insert(uuid, send);

    (uuid, recv)
  }

  pub async fn remove_session(&self, user: &Uuid, uuid: &Uuid) {
    if let Some(pair) = self.sessions.get(user) {
      pair.value().remove(uuid);
    }
  }
}

impl<T: UpdateMessage> Updater<T> {
  pub async fn broadcast(&self, msg: T) {
    let _ = self
      .0
      .send(UpdateTrigger {
        target: None,
        message: msg,
      })
      .await;
  }

  pub async fn send_to(&self, target: Uuid, msg: T) {
    let _ = self
      .0
      .send(UpdateTrigger {
        target: Some(target),
        message: msg,
      })
      .await;
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde::Deserialize;

  #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
  enum Msg {
    Ping(u8),
  }

  impl UpdateMessage for Msg {
    fn settings() -> Self {
      Msg::Ping(0)
    }
    fn group(_: Uuid) -> Self {
      Msg::Ping(1)
    }
    fn user(_: Uuid) -> Self {
      Msg::Ping(2)
    }
    fn user_permissions() -> Self {
      Msg::Ping(3)
    }
  }

  #[tokio::test]
  async fn test_send_to_targets_single_user() {
    let (state, updater) = UpdateState::<Msg>::init().await;
    let user = Uuid::new_v4();
    let other = Uuid::new_v4();

    let (_id, mut rx) = state.create_session(user).await;
    let (_id2, mut rx_other) = state.create_session(other).await;

    updater.send_to(user, Msg::Ping(42)).await;

    // The targeted user receives the message...
    assert_eq!(rx.recv().await, Some(Msg::Ping(42)));
    // ...and the other user does not (channel is still empty).
    assert!(rx_other.try_recv().is_err());
  }

  #[tokio::test]
  async fn test_broadcast_reaches_all_sessions() {
    let (state, updater) = UpdateState::<Msg>::init().await;
    let user = Uuid::new_v4();
    let (_a, mut rx_a) = state.create_session(user).await;
    let (_b, mut rx_b) = state.create_session(user).await;

    updater.broadcast(Msg::Ping(7)).await;

    assert_eq!(rx_a.recv().await, Some(Msg::Ping(7)));
    assert_eq!(rx_b.recv().await, Some(Msg::Ping(7)));
  }

  #[tokio::test]
  async fn test_remove_session_stops_delivery() {
    let (state, updater) = UpdateState::<Msg>::init().await;
    let user = Uuid::new_v4();
    let (id, mut rx) = state.create_session(user).await;

    state.remove_session(&user, &id).await;
    updater.send_to(user, Msg::Ping(1)).await;

    // After removal the (now-orphaned) receiver gets nothing.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert!(rx.try_recv().is_err());
  }
}
