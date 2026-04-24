#[cfg(feature = "backend")]
pub use axum;
#[cfg(feature = "backend")]
pub use axum_extra;
#[cfg(feature = "error")]
pub use eyre;
#[cfg(feature = "http")]
pub use http;

#[cfg(feature = "backend")]
pub mod backend;
#[cfg(feature = "db")]
pub mod db;
#[cfg(feature = "error")]
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
#[cfg(feature = "backend")]
pub use centaurus_derive::{Config, UpdateMessage};

// Used for re-reports required by macros
#[doc(hidden)]
pub mod private {
  pub use std::result::Result::Err;
}

pub const VERSION_HEADER_NAME: &str = "X-Api-Version";
