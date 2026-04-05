use std::{fmt::Debug, sync::Arc};

use aide::OperationIo;
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
}

#[derive(Clone, OperationIo)]
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

#[derive(Clone, OperationIo)]
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
