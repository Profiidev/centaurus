use axum::Extension;

use crate::backend::{BackendRouter, rewrite::proxy::ProxyExt};

pub fn frontend(router: BackendRouter) -> BackendRouter {
  #[cfg(not(debug_assertions))]
  let frontend_dir = env!("FRONTEND_DIR");

  #[cfg(not(debug_assertions))]
  let frontend_url = env!("FRONTEND_URL");
  #[cfg(debug_assertions)]
  let frontend_url = "http://frontend:5173/";

  #[cfg(not(debug_assertions))]
  let handle = tokio::process::Command::new("node")
    .arg(".")
    .current_dir(frontend_dir)
    .kill_on_drop(true)
    .spawn()
    .expect("Failed to start frontend server");

  router
    .proxy("/".into(), frontend_url.into())
    .layer(Extension(FrontendState {
      #[cfg(not(debug_assertions))]
      _handle: std::sync::Arc::new(handle),
    }))
}

#[derive(Clone, Debug)]
struct FrontendState {
  #[cfg(not(debug_assertions))]
  _handle: std::sync::Arc<tokio::process::Child>,
}
