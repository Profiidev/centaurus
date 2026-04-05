#[cfg(feature = "axum")]
pub use axum;
#[cfg(feature = "axum")]
pub use axum_extra;
#[cfg(feature = "error")]
pub use eyre;
#[cfg(feature = "http")]
pub use http;

#[cfg(feature = "axum")]
pub mod backend;
#[cfg(feature = "sea-orm")]
pub mod db;
#[cfg(feature = "error")]
pub mod error;
pub mod file;
#[cfg(feature = "gravatar")]
pub mod gravatar;
#[cfg(feature = "logging")]
pub mod logging;
#[cfg(feature = "lettre")]
pub mod mail;
pub mod req;
#[cfg(feature = "serde")]
pub mod serde;
pub mod state;

#[cfg(feature = "axum")]
pub use centaurus_derive::Config;
#[cfg(feature = "sea-orm")]
pub use centaurus_derive::Settings;

// Used for re-reports required by macros
#[doc(hidden)]
pub mod private {
  pub use std::result::Result::Err;
}
