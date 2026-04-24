use axum::Extension;

use crate::backend::{
  BackendRouter,
  endpoints::websocket::state::{UpdateMessage, UpdateState},
};

pub mod state;
mod updater;

pub fn router<T: UpdateMessage>() -> BackendRouter {
  BackendRouter::new().merge(updater::router::<T>())
}

pub async fn state<T: UpdateMessage>(router: BackendRouter) -> BackendRouter {
  let (state, updater) = UpdateState::<T>::init().await;

  router.layer(Extension(state)).layer(Extension(updater))
}
