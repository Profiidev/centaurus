pub mod config;
pub mod cors;
#[cfg(feature = "frontend")]
pub mod frontend;
pub mod init;
#[cfg(feature = "logging")]
pub mod logging;
#[cfg(feature = "metrics")]
pub mod metrics;
#[cfg(feature = "proxy")]
pub mod proxy;
pub mod rate_limiter;
pub mod router;
pub mod version;

#[cfg(not(feature = "openapi"))]
pub type BackendRouter = axum::Router;
#[cfg(feature = "openapi")]
pub type BackendRouter = aide::axum::ApiRouter;
