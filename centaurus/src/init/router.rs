use axum::Router;

#[cfg(feature = "metrics")]
use crate::init::metrics::init_metrics;
use crate::{config::BaseConfig, init::axum::add_base_layers, req::health};

pub async fn base_router(
  api_router: Router,
  config: &BaseConfig,
  #[cfg(feature = "metrics")] metrics_name: String,
  #[cfg(feature = "metrics")] extra_labels: Vec<(String, String)>,
) -> Router {
  #[cfg(feature = "metrics")]
  let handle = init_metrics(metrics_name.clone());

  #[cfg(feature = "frontend")]
  let mut router = crate::init::frontend::router();
  #[cfg(not(feature = "frontend"))]
  let mut router = Router::new();

  #[cfg(not(feature = "metrics"))]
  let sub_router = api_router.merge(health::router());
  #[cfg(feature = "metrics")]
  let mut sub_router = api_router.merge(health::router());

  #[cfg(feature = "metrics")]
  {
    use crate::init::metrics::metrics_route;

    sub_router = sub_router.metrics_route().await;
  }

  router = router
    .nest("/api", sub_router)
    .add_base_layers_filtered(config, |path| path.starts_with("/api"))
    .await;

  #[cfg(feature = "frontend")]
  {
    use crate::init::frontend::frontend;

    router = router.frontend().await;
  }

  #[cfg(feature = "metrics")]
  {
    use crate::init::metrics::metrics;

    router = router.metrics(metrics_name, handle, extra_labels).await;
  }

  router
}
