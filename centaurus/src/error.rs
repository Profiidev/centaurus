use std::{fmt::Debug, num::ParseIntError};

#[cfg(feature = "axum")]
use axum::{
  extract::{
    multipart::{MultipartError, MultipartRejection},
    rejection::BytesRejection,
  },
  response::{IntoResponse, Response},
};
#[cfg(feature = "axum")]
use axum_extra::typed_header::TypedHeaderRejection;
#[cfg(feature = "hmac")]
use hmac::digest::InvalidLength;
#[cfg(feature = "http")]
use http::StatusCode;

pub type Result<T> = std::result::Result<T, ErrorReport>;

#[macro_export]
macro_rules! anyhow {
  ($status:ident, $msg:literal) => {
    $crate::error::ErrorReport::new($crate::eyre::eyre!($msg), $crate::http::StatusCode::$status)
  };
  ($msg:literal) => {
    $crate::anyhow!(BAD_REQUEST, $msg)
  };
  ($status:ident, $err:expr) => {
    $crate::error::ErrorReport::new($crate::eyre::eyre!($err), $crate::http::StatusCode::$status)
  };
  ($err:expr) => {
    $crate::anyhow!(BAD_REQUEST, $err)
  };
  ($status:ident, $fmt:expr, $($arg:tt)*) => {
    $crate::error::ErrorReport::new($crate::eyre::eyre!($fmt, $($arg)*), $crate::http::StatusCode::$status)
  };
  ($fmt:expr, $($arg:tt)*) => {
    $crate::anyhow!(BAD_REQUEST, $fmt, $($arg)*)
  };
}

#[macro_export]
macro_rules! bail {
  ($status:ident, $msg:literal) => {
    return $crate::private::Err($crate::anyhow!($status, $msg));
  };
  ($msg:literal) => {
    return $crate::private::Err($crate::anyhow!($msg));
  };
  ($status:ident, $err:expr) => {
    return $crate::private::Err($crate::anyhow!($status, $err));
  };
  ($err:expr) => {
    return $crate::private::Err($crate::anyhow!($err));
  };
  ($status:ident, $fmt:expr, $($arg:tt)*) => {
    return $crate::private::Err($crate::anyhow!($status, $fmt, $($arg)*));
  };
  ($fmt:expr, $($arg:tt)*) => {
    return $crate::private::Err($crate::anyhow!($fmt, $($arg)*));
  };
}

#[macro_export]
macro_rules! impl_from_error {
  ($error:ty, $status:expr) => {
    impl From<$error> for ErrorReport {
      #[track_caller]
      fn from(value: $error) -> Self {
        Self {
          error: eyre::Report::new(value),
          #[cfg(feature = "http")]
          status: $status,
        }
      }
    }
  };
}

pub struct ErrorReport {
  error: eyre::Report,
  #[cfg(feature = "http")]
  status: StatusCode,
}

impl ErrorReport {
  pub fn new(error: eyre::Report, #[cfg(feature = "http")] status: StatusCode) -> Self {
    Self {
      error,
      #[cfg(feature = "http")]
      status,
    }
  }
}

impl_from_error!(std::io::Error, StatusCode::INTERNAL_SERVER_ERROR);
#[cfg(feature = "http")]
impl_from_error!(http::header::InvalidHeaderValue, StatusCode::BAD_REQUEST);
#[cfg(feature = "axum")]
impl_from_error!(TypedHeaderRejection, StatusCode::BAD_REQUEST);
#[cfg(feature = "axum")]
impl_from_error!(BytesRejection, StatusCode::BAD_REQUEST);
#[cfg(feature = "hmac")]
impl_from_error!(InvalidLength, StatusCode::BAD_REQUEST);
#[cfg(feature = "axum")]
impl_from_error!(MultipartRejection, StatusCode::BAD_REQUEST);
#[cfg(feature = "axum")]
impl_from_error!(MultipartError, StatusCode::BAD_REQUEST);
#[cfg(feature = "chrono")]
impl_from_error!(chrono::ParseError, StatusCode::BAD_REQUEST);
impl_from_error!(ParseIntError, StatusCode::BAD_REQUEST);
#[cfg(feature = "axum")]
impl_from_error!(serde_xml_rs::Error, StatusCode::BAD_REQUEST);
#[cfg(feature = "jsonwebtoken")]
impl_from_error!(jsonwebtoken::errors::Error, StatusCode::BAD_REQUEST);
#[cfg(feature = "sea-orm")]
impl_from_error!(sea_orm::DbErr, StatusCode::INTERNAL_SERVER_ERROR);
#[cfg(feature = "base64")]
impl_from_error!(base64::DecodeError, StatusCode::BAD_REQUEST);
#[cfg(feature = "rsa")]
impl_from_error!(rsa::Error, StatusCode::BAD_REQUEST);
#[cfg(feature = "argon2")]
impl_from_error!(argon2::password_hash::Error, StatusCode::BAD_REQUEST);

#[cfg(feature = "axum")]
impl IntoResponse for ErrorReport {
  fn into_response(self) -> Response {
    #[cfg(feature = "logging")]
    tracing::error!("{:?}", self.error);
    self.status.into_response()
  }
}

impl Debug for ErrorReport {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    <eyre::Report as Debug>::fmt(&self.error, f)
  }
}

impl From<eyre::Report> for ErrorReport {
  fn from(value: eyre::Report) -> Self {
    Self {
      error: value,
      #[cfg(feature = "http")]
      status: StatusCode::BAD_REQUEST,
    }
  }
}
