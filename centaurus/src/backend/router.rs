use axum::{Extension, Router};

#[cfg(feature = "metrics")]
use crate::backend::metrics::init_metrics;
use crate::{
  backend::{config::Config, init::add_base_layers, rate_limiter::RateLimiter},
  req::health,
};

pub async fn base_router<R, S, C, F>(router: R, state: S, config: C) -> Router
where
  R: FnOnce(&mut RateLimiter) -> Router,
  S: FnOnce(Router, &C) -> F,
  F: Future<Output = Router>,
  C: Config,
{
  #[cfg(feature = "metrics")]
  let metrics_config = config.metrics();
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
    .add_base_layers_filtered(config.base(), |path| path.starts_with("/api"))
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

  state(router, &config).await.layer(Extension(config))
}
