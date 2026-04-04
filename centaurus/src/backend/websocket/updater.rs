use aide::axum::{ApiRouter, IntoApiResponse};
use axum::{
  extract::{
    WebSocketUpgrade,
    ws::{Message, WebSocket},
  },
  routing::get,
};
use futures_util::StreamExt;
use tokio::sync::mpsc::Receiver;
use uuid::Uuid;

use crate::{
  auth::jwt_auth::JwtAuth,
  backend::websocket::state::{UpdateMessage, UpdateState},
};

pub fn router<T: UpdateMessage>() -> ApiRouter {
  ApiRouter::new().route("/updater", get(update::<T>))
}

async fn update<T: UpdateMessage>(
  auth: JwtAuth,
  ws: WebSocketUpgrade,
  state: UpdateState<T>,
) -> impl IntoApiResponse {
  let (uuid, recv) = state.create_session(auth.user_id).await;

  ws.on_upgrade(move |socket| handle_socket(socket, auth.user_id, uuid, recv, state))
}

async fn handle_socket<T: UpdateMessage>(
  mut socket: WebSocket,
  user: Uuid,
  uuid: Uuid,
  mut recv: Receiver<T>,
  state: UpdateState<T>,
) {
  loop {
    tokio::select! {
      update = recv.recv() => {
        match update {
          Some(message) => {
            let message = serde_json::to_string(&message).unwrap();
            let message = Message::Text(message.into());

            let _ = socket.send(message).await;
          }
          None => {
            state.remove_session(&user, &uuid).await;
            break;
          }
        }
      }

      ws_msg = socket.next() => {
        if let Some(Ok(Message::Close(_)) | Err(_)) | None = ws_msg {
          state.remove_session(&user, &uuid).await;
          break;
        }
      }
    }
  }
}
