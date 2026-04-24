#[cfg(feature = "error")]
pub use eyre;

#[cfg(feature = "db")]
pub mod db;
pub mod error;
pub mod file;
#[cfg(feature = "gravatar")]
pub mod gravatar;
#[cfg(feature = "logging")]
pub mod logging;
#[cfg(feature = "mail")]
pub mod mail;
#[cfg(feature = "serde")]
pub mod serde;

#[cfg(feature = "db")]
pub use centaurus_derive::Settings;

// Used for re-reports required by macros
#[doc(hidden)]
pub mod private {
  pub use std::result::Result::Err;
}

pub const VERSION_HEADER_NAME: &str = "X-Api-Version";

/*
#[cfg(feature = "axum")]
pub use axum;
#[cfg(feature = "axum")]
pub use axum_extra;
#[cfg(feature = "http")]
pub use http;

#[cfg(feature = "axum")]
pub mod backend;
#[cfg(feature = "sea-orm")]
pub mod db;
#[cfg(feature = "error")]
pub mod error;
pub mod file;
pub mod req;
pub mod state;

#[cfg(feature = "axum")]
pub use centaurus_derive::Config;
#[cfg(all(feature = "auth", feature = "axum"))]
pub use centaurus_derive::UpdateMessage;


pub const VERSION_HEADER_NAME: &str = "X-Api-Version";
*/
