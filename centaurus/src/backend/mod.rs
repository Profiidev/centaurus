#[cfg(feature = "auth")]
pub mod auth;
pub mod config;
pub mod endpoints;
pub mod init;
pub mod middleware;
pub mod request;
pub mod rewrite;
pub mod router;

#[cfg(not(feature = "openapi"))]
pub type BackendRouter = axum::Router;
#[cfg(feature = "openapi")]
pub type BackendRouter = aide::axum::ApiRouter;
