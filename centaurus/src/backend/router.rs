use axum::Router;

#[cfg(feature = "metrics")]
use crate::{backend::metrics::init_metrics, config::MetricsConfig};
use crate::{
  backend::{init::add_base_layers, rate_limiter::RateLimiter},
  config::BaseConfig,
  req::health,
};

pub async fn base_router<R: FnOnce(&mut RateLimiter) -> Router>(
  router: R,
  config: &BaseConfig,
  #[cfg(feature = "metrics")] metrics_config: &MetricsConfig,
) -> Router {
  #[cfg(feature = "metrics")]
  let handle = init_metrics(metrics_config.metrics_name.clone());

  let mut rate_limiter = RateLimiter::default();
  let api_router = router(&mut rate_limiter);
  rate_limiter.init();

  #[cfg(feature = "frontend")]
  let mut router = crate::backend::frontend::router();
  #[cfg(not(feature = "frontend"))]
  let mut router = Router::new();

  #[cfg(not(feature = "metrics"))]
  let sub_router = api_router.merge(health::router());
  #[cfg(feature = "metrics")]
  let mut sub_router = api_router.merge(health::router());

  #[cfg(feature = "metrics")]
  {
    use crate::backend::metrics::metrics_route;
    sub_router = metrics_route(sub_router);
  }

  router = router
    .nest("/api", sub_router)
    .add_base_layers_filtered(config, |path| path.starts_with("/api"))
    .await;

  #[cfg(feature = "frontend")]
  {
    use crate::backend::frontend::frontend;
    router = frontend(router);
  }

  #[cfg(feature = "metrics")]
  {
    use crate::backend::metrics::metrics;

    router = metrics(
      router,
      metrics_config.metrics_name.clone(),
      handle,
      metrics_config.extra_labels.clone(),
    );
  }

  router
}
