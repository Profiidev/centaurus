#[cfg(feature = "auth")]
pub mod auth;
pub mod config;
#[cfg(all(feature = "sea-orm", feature = "auth"))]
pub mod group;
pub mod init;
#[cfg(all(feature = "lettre", feature = "auth"))]
pub mod mail;
pub mod middleware;
pub mod res;
pub mod rewrite;
pub mod router;
#[cfg(all(
  feature = "sea-orm",
  feature = "auth",
  feature = "gravatar",
  feature = "image"
))]
pub mod settings;
#[cfg(all(
  feature = "sea-orm",
  feature = "auth",
  feature = "gravatar",
  feature = "image"
))]
pub mod setup;
#[cfg(all(
  feature = "sea-orm",
  feature = "auth",
  feature = "gravatar",
  feature = "image"
))]
pub mod user;
#[cfg(all(feature = "auth", feature = "sea-orm"))]
pub mod websocket;

#[cfg(not(feature = "openapi"))]
pub type BackendRouter = axum::Router;
#[cfg(feature = "openapi")]
pub type BackendRouter = aide::axum::ApiRouter;
