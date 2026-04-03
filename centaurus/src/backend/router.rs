use axum::Extension;
use tower::ServiceBuilder;

#[cfg(feature = "metrics")]
use crate::backend::metrics::init_metrics;
use crate::{
  backend::{BackendRouter, config::Config, rate_limiter::RateLimiter},
  req::health,
};

pub async fn base_router<R, S, C, F>(router: R, state: S, config: C) -> BackendRouter
where
  R: FnOnce(&mut RateLimiter) -> BackendRouter,
  S: FnOnce(BackendRouter, &C) -> F,
  F: Future<Output = BackendRouter>,
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
  let mut router = BackendRouter::new();

  #[cfg(not(feature = "metrics"))]
  let sub_router = api_router.merge(health::router());
  #[cfg(feature = "metrics")]
  let mut sub_router = api_router.merge(health::router());

  #[cfg(feature = "metrics")]
  {
    use crate::backend::metrics::metrics_route;
    sub_router = metrics_route(sub_router);
  }

  router = router.nest("/api", sub_router).layer(
    ServiceBuilder::new()
      .layer(super::cors::cors(config.base()).expect("Faield to build CORS layer")),
  );

  #[cfg(feature = "frontend")]
  {
    use super::logging::logging;
    router = logging(router, |path| path.starts_with("/api"));
  }

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
