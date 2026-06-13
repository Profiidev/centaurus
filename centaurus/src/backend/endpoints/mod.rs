#[cfg(feature = "endpoints")]
pub mod group;
pub mod health;
#[cfg(feature = "endpoints")]
pub mod mail;
#[cfg(feature = "endpoints")]
pub mod settings;
#[cfg(feature = "endpoints")]
pub mod setup;
#[cfg(feature = "endpoints")]
pub mod user;
#[cfg(feature = "endpoints")]
pub mod websocket;

#[cfg(all(test, feature = "endpoints"))]
mod integration_tests;
