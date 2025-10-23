use axum::response::{IntoResponse, Response};
use http::{HeaderValue, StatusCode, header::LOCATION};

#[derive(Debug, Clone)]
pub struct Redirect {
  status_code: StatusCode,
  location: HeaderValue,
}

impl IntoResponse for Redirect {
  fn into_response(self) -> Response {
    (self.status_code, [(LOCATION, self.location)]).into_response()
  }
}

impl Redirect {
  pub fn found(uri: String) -> Self {
    Self::with_status_code(StatusCode::FOUND, &uri)
  }

  fn with_status_code(status_code: StatusCode, uri: &str) -> Self {
    assert!(
      status_code.is_redirection(),
      "not a redirection status code"
    );

    Self {
      status_code,
      location: HeaderValue::try_from(uri).expect("URI isn't a valid header value"),
    }
  }
}
