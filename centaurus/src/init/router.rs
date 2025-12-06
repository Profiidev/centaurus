use axum::Router;

use crate::{config::BaseConfig, init::axum::add_base_layers, req::health};

pub async fn base_router(api_router: Router, config: &BaseConfig) -> Router {
  #[cfg(feature = "frontend")]
  let router = crate::init::frontend::router();
  #[cfg(not(feature = "frontend"))]
  let router = Router::new();

  #[cfg(not(feature = "metrics"))]
  let sub_router = api_router.merge(health::router());
  #[cfg(feature = "metrics")]
  let mut sub_router = api_router.merge(health::router());

  #[cfg(feature = "metrics")]
  {
    use crate::init::metrics::metrics_route;

    sub_router = sub_router.metrics_route().await;
  }

  router
    .nest("/api", sub_router)
    .add_base_layers_filtered(config, |path| path.starts_with("/api"))
    .await
}
