#[cfg(feature = "auth")]
pub mod auth;
pub mod config;
pub mod init;
pub mod middleware;
pub mod res;
pub mod rewrite;
pub mod router;
pub mod websocket;

#[cfg(not(feature = "openapi"))]
pub type BackendRouter = axum::Router;
#[cfg(feature = "openapi")]
pub type BackendRouter = aide::axum::ApiRouter;
