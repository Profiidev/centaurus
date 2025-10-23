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
    impl From<$error> for $crate::error::ErrorReport {
      #[track_caller]
      fn from(value: $error) -> Self {
        Self {
          error: $crate::eyre::Report::new(value),
          status: $status,
        }
      }
    }
  };
}

pub struct ErrorReport {
  pub error: eyre::Report,
  #[cfg(feature = "http")]
  pub status: StatusCode,
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
impl_from_error!(InvalidLength, StatusCode::INTERNAL_SERVER_ERROR);
#[cfg(feature = "axum")]
impl_from_error!(MultipartRejection, StatusCode::BAD_REQUEST);
#[cfg(feature = "axum")]
impl_from_error!(MultipartError, StatusCode::BAD_REQUEST);
#[cfg(feature = "chrono")]
impl_from_error!(chrono::ParseError, StatusCode::INTERNAL_SERVER_ERROR);
impl_from_error!(ParseIntError, StatusCode::BAD_REQUEST);
#[cfg(feature = "axum")]
impl_from_error!(serde_xml_rs::Error, StatusCode::BAD_REQUEST);
#[cfg(feature = "axum")]
impl_from_error!(serde_json::Error, StatusCode::BAD_REQUEST);
#[cfg(feature = "jsonwebtoken")]
impl_from_error!(jsonwebtoken::errors::Error, StatusCode::BAD_REQUEST);
#[cfg(feature = "sea-orm")]
impl_from_error!(sea_orm::DbErr, StatusCode::INTERNAL_SERVER_ERROR);
#[cfg(feature = "base64")]
impl_from_error!(base64::DecodeError, StatusCode::INTERNAL_SERVER_ERROR);
#[cfg(feature = "rsa")]
impl_from_error!(rsa::Error, StatusCode::INTERNAL_SERVER_ERROR);
#[cfg(feature = "argon2")]
impl_from_error!(
  argon2::password_hash::Error,
  StatusCode::INTERNAL_SERVER_ERROR
);
#[cfg(feature = "uuid")]
impl_from_error!(uuid::Error, StatusCode::INTERNAL_SERVER_ERROR);
#[cfg(feature = "reqwest")]
impl_from_error!(reqwest::Error, StatusCode::INTERNAL_SERVER_ERROR);
#[cfg(feature = "lettre")]
impl_from_error!(lettre::error::Error, StatusCode::INTERNAL_SERVER_ERROR);
#[cfg(feature = "webauthn")]
impl_from_error!(
  webauthn_rs_core::error::WebauthnError,
  StatusCode::BAD_REQUEST
);
#[cfg(feature = "image")]
impl_from_error!(image::error::ImageError, StatusCode::INTERNAL_SERVER_ERROR);

#[cfg(feature = "http")]
pub trait ErrorReportStatusExt<T> {
  fn status(self, status: StatusCode) -> Result<T>;
  fn status_context(self, status: StatusCode, msg: &str) -> Result<T>;
}

#[cfg(feature = "http")]
impl<T, E: std::error::Error + Send + Sync + 'static> ErrorReportStatusExt<T>
  for std::result::Result<T, E>
{
  fn status(self, status: StatusCode) -> Result<T> {
    self.map_err(|e| ErrorReport::new(eyre::Report::new(e), status))
  }

  fn status_context(self, status: StatusCode, msg: &str) -> Result<T> {
    self.map_err(|e| ErrorReport::new(eyre::Report::new(e).wrap_err(msg.to_string()), status))
  }
}

#[cfg(feature = "http")]
impl<T> ErrorReportStatusExt<T> for Option<T> {
  fn status(self, status: StatusCode) -> Result<T> {
    self.ok_or_else(|| ErrorReport::new(eyre::Report::msg("Option is None"), status))
  }

  fn status_context(self, status: StatusCode, msg: &str) -> Result<T> {
    self.ok_or_else(|| ErrorReport::new(eyre::Report::msg(msg.to_string()), status))
  }
}

pub trait ErrorReportExt<T> {
  fn context(self, msg: &str) -> Result<T>;
}

impl<T, E: Into<ErrorReport>> ErrorReportExt<T> for std::result::Result<T, E> {
  fn context(self, msg: &str) -> Result<T> {
    self.map_err(|e| {
      let mut e = e.into();
      e.error = e.error.wrap_err(msg.to_string());
      e
    })
  }
}

#[cfg(feature = "axum")]
impl IntoResponse for ErrorReport {
  fn into_response(self) -> Response {
    #[cfg(feature = "logging")]
    if self.status.is_server_error() {
      tracing::error!("{:?}", self.error);
    } else {
      tracing::warn!("{:?}", self.error);
    }
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
