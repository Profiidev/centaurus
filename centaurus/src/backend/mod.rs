#[cfg(feature = "auth")]
pub mod auth;
pub mod config;
pub mod init;
#[cfg(feature = "lettre")]
pub mod mail;
pub mod middleware;
pub mod res;
pub mod rewrite;
pub mod router;
#[cfg(feature = "sea-orm")]
pub mod settings;
#[cfg(feature = "auth")]
pub mod websocket;

#[cfg(not(feature = "openapi"))]
pub type BackendRouter = axum::Router;
#[cfg(feature = "openapi")]
pub type BackendRouter = aide::axum::ApiRouter;
