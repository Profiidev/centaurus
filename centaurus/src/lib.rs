#[cfg(feature = "argon2")]
pub use argon2;
#[cfg(feature = "axum")]
pub use axum;
#[cfg(feature = "axum-extra")]
pub use axum_extra;
#[cfg(feature = "base64")]
pub use base64;
#[cfg(feature = "chrono")]
pub use chrono;
#[cfg(feature = "config")]
pub use clap;
#[cfg(feature = "logging")]
pub use color_eyre;
#[cfg(feature = "error")]
pub use eyre;
#[cfg(feature = "hmac")]
pub use hmac;
#[cfg(feature = "http")]
pub use http;
#[cfg(feature = "jsonwebtoken")]
pub use jsonwebtoken;
#[cfg(feature = "rsa")]
pub use rsa;
#[cfg(feature = "sea-orm")]
pub use sea_orm;
#[cfg(feature = "xml")]
pub use serde_xml_rs;
#[cfg(feature = "tracing")]
pub use tracing;
#[cfg(feature = "logging")]
pub use tracing_error;
#[cfg(feature = "tracing")]
pub use tracing_subscriber;

#[cfg(feature = "config")]
pub mod config;
#[cfg(feature = "error")]
pub mod error;
pub mod file;
pub mod init;
pub mod state;

#[cfg(all(feature = "axum", feature = "http"))]
pub use centaurus_derive::FromReqExtension;

// Used for re-reports required by macros
#[doc(hidden)]
pub mod private {
  pub use std::result::Result::Err;
}
