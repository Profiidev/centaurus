use std::{
  fmt::{Debug, Display},
  num::ParseIntError,
};

#[cfg(feature = "backend")]
use axum::{
  extract::{
    multipart::{MultipartError, MultipartRejection},
    rejection::BytesRejection,
  },
  response::{IntoResponse, Response},
};
#[cfg(feature = "backend")]
use axum_extra::typed_header::TypedHeaderRejection;
#[cfg(feature = "hmac")]
use hmac::digest::InvalidLength;
#[cfg(feature = "http")]
use http::StatusCode;

pub type Result<T> = std::result::Result<T, ErrorReport>;

#[cfg(feature = "http")]
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

#[cfg(not(feature = "http"))]
#[macro_export]
macro_rules! anyhow {
  ($msg:literal) => {
    $crate::error::ErrorReport::new($crate::eyre::eyre!($msg))
  };
  ($err:expr) => {
    $crate::error::ErrorReport::new($crate::eyre::eyre!($err))
  };
  ($fmt:expr, $($arg:tt)*) => {
    $crate::error::ErrorReport::new($crate::eyre::eyre!($fmt, $($arg)*))
  };
}

#[cfg(feature = "http")]
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

#[cfg(not(feature = "http"))]
#[macro_export]
macro_rules! bail {
  ($msg:literal) => {
    return $crate::private::Err($crate::anyhow!($msg));
  };
  ($err:expr) => {
    return $crate::private::Err($crate::anyhow!($err));
  };
  ($fmt:expr, $($arg:tt)*) => {
    return $crate::private::Err($crate::anyhow!($fmt, $($arg)*));
  };
}

#[cfg(feature = "http")]
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

#[cfg(not(feature = "http"))]
#[macro_export]
macro_rules! impl_from_error {
  ($error:ty, $rest:expr) => {
    impl From<$error> for $crate::error::ErrorReport {
      #[track_caller]
      fn from(value: $error) -> Self {
        Self {
          error: $crate::eyre::Report::new(value),
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
#[cfg(feature = "chrono")]
impl_from_error!(chrono::ParseError, StatusCode::INTERNAL_SERVER_ERROR);
impl_from_error!(ParseIntError, StatusCode::BAD_REQUEST);
#[cfg(feature = "url")]
impl_from_error!(url::ParseError, StatusCode::BAD_REQUEST);
#[cfg(feature = "docker")]
impl_from_error!(bollard::errors::Error, StatusCode::INTERNAL_SERVER_ERROR);
#[cfg(feature = "image")]
impl_from_error!(image::error::ImageError, StatusCode::INTERNAL_SERVER_ERROR);
#[cfg(feature = "webauthn")]
impl_from_error!(
  webauthn_rs_core::error::WebauthnError,
  StatusCode::BAD_REQUEST
);
#[cfg(feature = "uuid")]
impl_from_error!(uuid::Error, StatusCode::INTERNAL_SERVER_ERROR);
#[cfg(feature = "base64")]
impl_from_error!(base64::DecodeError, StatusCode::INTERNAL_SERVER_ERROR);
#[cfg(feature = "rsa")]
impl_from_error!(rsa::Error, StatusCode::INTERNAL_SERVER_ERROR);
#[cfg(feature = "argon2")]
impl_from_error!(
  argon2::password_hash::Error,
  StatusCode::INTERNAL_SERVER_ERROR
);
#[cfg(feature = "jsonwebtoken")]
impl_from_error!(jsonwebtoken::errors::Error, StatusCode::BAD_REQUEST);
#[cfg(feature = "db")]
impl_from_error!(sea_orm::DbErr, StatusCode::INTERNAL_SERVER_ERROR);
#[cfg(feature = "serde_xml")]
impl_from_error!(serde_xml_rs::Error, StatusCode::BAD_REQUEST);
#[cfg(feature = "serde_json")]
impl_from_error!(serde_json::Error, StatusCode::BAD_REQUEST);

#[cfg(feature = "http")]
impl_from_error!(http::header::InvalidHeaderValue, StatusCode::BAD_REQUEST);
#[cfg(feature = "http")]
impl_from_error!(http::header::InvalidHeaderName, StatusCode::BAD_REQUEST);
#[cfg(feature = "backend")]
impl_from_error!(TypedHeaderRejection, StatusCode::BAD_REQUEST);
#[cfg(feature = "backend")]
impl_from_error!(BytesRejection, StatusCode::BAD_REQUEST);
#[cfg(feature = "hmac")]
impl_from_error!(InvalidLength, StatusCode::INTERNAL_SERVER_ERROR);
#[cfg(feature = "backend")]
impl_from_error!(MultipartRejection, StatusCode::BAD_REQUEST);
#[cfg(feature = "backend")]
impl_from_error!(MultipartError, StatusCode::BAD_REQUEST);
#[cfg(feature = "reqwest")]
impl_from_error!(reqwest::Error, StatusCode::INTERNAL_SERVER_ERROR);
#[cfg(feature = "mail")]
impl_from_error!(lettre::error::Error, StatusCode::INTERNAL_SERVER_ERROR);
#[cfg(feature = "k8s")]
impl_from_error!(kube::Error, StatusCode::INTERNAL_SERVER_ERROR);
#[cfg(feature = "k8s")]
impl_from_error!(
  kube::core::request::Error,
  StatusCode::INTERNAL_SERVER_ERROR
);
#[cfg(feature = "k8s")]
impl_from_error!(
  kube::runtime::watcher::Error,
  StatusCode::INTERNAL_SERVER_ERROR
);
#[cfg(feature = "k8s")]
impl_from_error!(
  kube::runtime::wait::Error,
  StatusCode::INTERNAL_SERVER_ERROR
);
#[cfg(feature = "k8s")]
impl_from_error!(
  kube::runtime::finalizer::Error<ErrorReport>,
  StatusCode::INTERNAL_SERVER_ERROR
);

#[cfg(feature = "http")]
pub trait ErrorReportStatusExt<T> {
  #[track_caller]
  fn status(self, status: StatusCode) -> Result<T>;
  #[track_caller]
  fn status_context(self, status: StatusCode, msg: &str) -> Result<T>;
}

#[cfg(feature = "http")]
impl<T, E: std::error::Error + Send + Sync + 'static> ErrorReportStatusExt<T>
  for std::result::Result<T, E>
{
  #[track_caller]
  fn status(self, status: StatusCode) -> Result<T> {
    // closures can not be used with track_caller
    match self {
      Ok(v) => Ok(v),
      Err(e) => Err(ErrorReport::new(eyre::Report::new(e), status)),
    }
  }

  #[track_caller]
  fn status_context(self, status: StatusCode, msg: &str) -> Result<T> {
    // closures can not be used with track_caller
    match self {
      Ok(v) => Ok(v),
      Err(e) => Err(ErrorReport::new(
        eyre::Report::new(e).wrap_err(msg.to_string()),
        status,
      )),
    }
  }
}

#[cfg(feature = "http")]
impl<T> ErrorReportStatusExt<T> for Option<T> {
  #[track_caller]
  fn status(self, status: StatusCode) -> Result<T> {
    // closures can not be used with track_caller
    match self {
      Some(v) => Ok(v),
      None => Err(ErrorReport::new(
        eyre::Report::msg("Option is None"),
        status,
      )),
    }
  }

  #[track_caller]
  fn status_context(self, status: StatusCode, msg: &str) -> Result<T> {
    // closures can not be used with track_caller
    match self {
      Some(v) => Ok(v),
      None => Err(ErrorReport::new(eyre::Report::msg(msg.to_string()), status)),
    }
  }
}

pub trait ErrorReportExt<T> {
  #[track_caller]
  fn context(self, msg: &str) -> Result<T>;
}

impl<T, E: Into<ErrorReport>> ErrorReportExt<T> for std::result::Result<T, E> {
  #[track_caller]
  fn context(self, msg: &str) -> Result<T> {
    // closures can not be used with track_caller
    match self {
      Ok(v) => Ok(v),
      Err(e) => {
        let mut e = e.into();
        e.error = e.error.wrap_err(msg.to_string());
        Err(e)
      }
    }
  }
}

#[cfg(feature = "backend")]
impl IntoResponse for ErrorReport {
  fn into_response(self) -> Response {
    #[cfg(feature = "logging")]
    if self.status.is_server_error() {
      tracing::error!("{:?}", self.error);
    } else {
      tracing::debug!("{:?}", self.error);
    }
    self.status.into_response()
  }
}

impl Debug for ErrorReport {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    <eyre::Report as Debug>::fmt(&self.error, f)
  }
}

impl Display for ErrorReport {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    <eyre::Report as Display>::fmt(&self.error, f)
  }
}

impl std::error::Error for ErrorReport {}

impl From<eyre::Report> for ErrorReport {
  fn from(value: eyre::Report) -> Self {
    Self {
      error: value,
      #[cfg(feature = "http")]
      status: StatusCode::BAD_REQUEST,
    }
  }
}

#[cfg(feature = "openapi")]
impl aide::OperationOutput for ErrorReport {
  type Inner = ErrorReport;

  fn inferred_responses(
    _ctx: &mut aide::generate::GenContext,
    _operation: &mut aide::openapi::Operation,
  ) -> Vec<(Option<aide::openapi::StatusCode>, aide::openapi::Response)> {
    fn empty() -> aide::openapi::Response {
      aide::openapi::Response {
        description: "An error occurred".to_string(),
        ..Default::default()
      }
    }

    vec![
      (Some(aide::openapi::StatusCode::Range(4)), empty()),
      (Some(aide::openapi::StatusCode::Range(5)), empty()),
    ]
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[cfg(feature = "http")]
  #[test]
  fn test_anyhow_macro() {
    let report = anyhow!("test error");
    assert_eq!(report.status, StatusCode::BAD_REQUEST);
    assert_eq!(format!("{}", report), "test error");
  }

  #[cfg(feature = "http")]
  #[test]
  fn test_bail_macro() {
    fn failing_func() -> Result<()> {
      bail!(INTERNAL_SERVER_ERROR, "failed");
    }

    let res = failing_func();
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
  }

  #[cfg(feature = "http")]
  #[test]
  fn test_error_report_status_ext() {
    let res: std::result::Result<i32, std::io::Error> =
      Err(std::io::Error::new(std::io::ErrorKind::Other, "oh no"));
    let report = res.status(StatusCode::NOT_FOUND).unwrap_err();
    assert_eq!(report.status, StatusCode::NOT_FOUND);
  }

  #[cfg(feature = "http")]
  #[test]
  fn test_status_passes_ok_through() {
    let res: std::result::Result<i32, std::io::Error> = Ok(5);
    assert_eq!(res.status(StatusCode::NOT_FOUND).unwrap(), 5);
  }

  #[cfg(feature = "http")]
  #[test]
  fn test_status_context_wraps_message() {
    let res: std::result::Result<(), std::io::Error> = Err(std::io::Error::other("inner"));
    let report = res
      .status_context(StatusCode::NOT_FOUND, "outer context")
      .unwrap_err();
    assert_eq!(report.status, StatusCode::NOT_FOUND);
    // The context message is prepended to the underlying error.
    assert!(format!("{}", report).contains("outer context"));
  }

  #[cfg(feature = "http")]
  #[test]
  fn test_option_status_ext() {
    let none: Option<i32> = None;
    assert_eq!(
      none.status(StatusCode::NOT_FOUND).unwrap_err().status,
      StatusCode::NOT_FOUND
    );
    let some: Option<i32> = Some(9);
    assert_eq!(some.status(StatusCode::NOT_FOUND).unwrap(), 9);

    let none: Option<i32> = None;
    let report = none
      .status_context(StatusCode::FORBIDDEN, "missing")
      .unwrap_err();
    assert_eq!(report.status, StatusCode::FORBIDDEN);
    assert!(format!("{}", report).contains("missing"));
  }

  #[test]
  fn test_context_ext_wraps() {
    let res: std::result::Result<(), std::io::Error> = Err(std::io::Error::other("root cause"));
    let report = res.context("while doing thing").unwrap_err();
    let rendered = format!("{}", report);
    // wrap_err makes the new message the outermost display value.
    assert!(rendered.contains("while doing thing"));
  }

  #[cfg(feature = "http")]
  #[test]
  fn test_from_io_error_is_internal() {
    // io::Error maps to 500 via impl_from_error.
    let report: ErrorReport = std::io::Error::other("boom").into();
    assert_eq!(report.status, StatusCode::INTERNAL_SERVER_ERROR);
  }

  #[cfg(feature = "http")]
  #[test]
  fn test_from_parse_int_is_bad_request() {
    let err = "x".parse::<i32>().unwrap_err();
    let report: ErrorReport = err.into();
    assert_eq!(report.status, StatusCode::BAD_REQUEST);
  }

  #[cfg(feature = "http")]
  #[test]
  fn test_from_eyre_report_defaults_bad_request() {
    let report: ErrorReport = eyre::eyre!("plain").into();
    assert_eq!(report.status, StatusCode::BAD_REQUEST);
  }

  #[cfg(feature = "backend")]
  #[test]
  fn test_into_response_uses_status() {
    let report = anyhow!(FORBIDDEN, "nope");
    assert_eq!(report.into_response().status(), StatusCode::FORBIDDEN);
  }

  #[cfg(feature = "http")]
  #[test]
  fn test_anyhow_with_explicit_status_and_format() {
    let report = anyhow!(NOT_FOUND, "missing {}", 42);
    assert_eq!(report.status, StatusCode::NOT_FOUND);
    assert_eq!(format!("{}", report), "missing 42");
  }
}
