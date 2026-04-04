pub mod config;
pub mod init;
pub mod middleware;
pub mod rewrite;
pub mod router;
pub mod websocket;

#[cfg(not(feature = "openapi"))]
pub type BackendRouter = axum::Router;
#[cfg(feature = "openapi")]
pub type BackendRouter = aide::axum::ApiRouter;
