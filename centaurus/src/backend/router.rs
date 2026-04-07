use axum::{Extension, Router};
use tower::ServiceBuilder;

#[cfg(feature = "metrics")]
use crate::backend::middleware::metrics::init_metrics;
use crate::{
  backend::{BackendRouter, config::Config, middleware::rate_limiter::RateLimiter},
  req::health,
};

pub async fn build_router<R, S, C, F>(router: R, state: S, config: C) -> Router
where
  R: FnOnce(&mut RateLimiter) -> BackendRouter,
  S: FnOnce(BackendRouter, C) -> F,
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

  let mut router = BackendRouter::new();

  #[cfg(not(feature = "metrics"))]
  let sub_router = api_router.merge(health::router());
  #[cfg(feature = "metrics")]
  let mut sub_router = api_router.merge(health::router());

  #[cfg(feature = "metrics")]
  let mut metrics_router = None;
  #[cfg(feature = "metrics")]
  {
    use crate::backend::middleware::metrics::metrics_route;
    if metrics_config.metrics_enabled {
      if let Some(port) = metrics_config.metrics_port {
        use crate::backend::init::listener_setup;

        let listener = listener_setup(port).await;
        metrics_router = Some((metrics_route(BackendRouter::new(), "/"), listener));
      } else {
        sub_router = metrics_route(sub_router, "/metrics");
      }
    }
  }

  router = router.nest("/api", sub_router).layer(
    ServiceBuilder::new()
      .layer(super::middleware::cors::cors(config.base()).expect("Faield to build CORS layer")),
  );

  #[cfg(feature = "logging")]
  {
    use super::middleware::logging::logging;
    router = logging(router, |path| path.starts_with("/api"));
  }

  #[cfg(feature = "frontend")]
  {
    use crate::backend::rewrite::frontend::frontend;
    router = frontend(router);
  }

  #[cfg(feature = "metrics")]
  {
    use crate::backend::middleware::metrics::metrics;

    if metrics_config.metrics_enabled {
      if let Some((mut metrics_router, listener)) = metrics_router {
        use crate::backend::middleware::metrics::metrics_middleware;
        router = metrics_middleware(
          router,
          metrics_config.metrics_name.clone(),
          metrics_config.extra_labels.clone(),
        );

        metrics_router = metrics(metrics_router, metrics_config.metrics_name.clone(), handle);

        tokio::spawn(async move {
          use crate::backend::init::run_app;
          use tracing::info;

          info!("Starting metrics server.");
          run_app(listener, metrics_router).await;
        });
      } else {
        use crate::backend::middleware::metrics::metrics_middleware;

        router = metrics_middleware(
          router,
          metrics_config.metrics_name.clone(),
          metrics_config.extra_labels.clone(),
        );
        router = metrics(router, metrics_config.metrics_name.clone(), handle);
      }
    }
  }

  #[cfg(feature = "config_site")]
  {
    router = router.layer(Extension(config.site().clone()));
  }

  router = state(router, config.clone()).await.layer(Extension(config));

  #[cfg(feature = "openapi")]
  {
    let mut api = aide::openapi::OpenApi::default();
    router
      .route(
        "/swagger",
        aide::swagger::Swagger::new("/openapi.json").axum_route(),
      )
      .route("/openapi.json", axum::routing::get(api_spec))
      .finish_api(&mut api)
      .layer(Extension(api))
  }

  #[cfg(not(feature = "openapi"))]
  router
}

#[cfg(feature = "openapi")]
async fn api_spec(
  Extension(api): Extension<aide::openapi::OpenApi>,
) -> axum::Json<aide::openapi::OpenApi> {
  axum::Json(api)
}
