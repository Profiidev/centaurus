use axum::{
  body::Bytes,
  extract::{FromRequest, OptionalFromRequest, Request, rejection::BytesRejection},
  response::{IntoResponse, Response},
};
use http::{StatusCode, header::CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug)]
pub struct Xml<T>(pub T);

impl<T: for<'de> Deserialize<'de>> Xml<T> {
  pub fn from_slice(slice: &[u8]) -> Result<Self, serde_xml_rs::Error> {
    Ok(Xml(serde_xml_rs::from_reader(slice)?))
  }
}

impl<T: Serialize> Xml<T> {
  pub fn to_slice(&self) -> Result<Vec<u8>, serde_xml_rs::Error> {
    let mut buf = Vec::new();
    serde_xml_rs::to_writer(&mut buf, &self.0)?;
    Ok(buf)
  }
}

#[derive(Error, Debug)]
pub enum XmlRejection {
  #[error(transparent)]
  BytesRejection(#[from] BytesRejection),
  #[error(transparent)]
  InvalidXml(#[from] serde_xml_rs::Error),
}

impl IntoResponse for XmlRejection {
  fn into_response(self) -> axum::response::Response {
    match self {
      XmlRejection::BytesRejection(rej) => rej.into_response(),
      XmlRejection::InvalidXml(_) => (StatusCode::BAD_REQUEST).into_response(),
    }
  }
}

impl<T: Serialize> IntoResponse for Xml<T> {
  fn into_response(self) -> Response {
    match self.to_slice() {
      Ok(body) => (StatusCode::OK, [(CONTENT_TYPE, "application/xml")], body).into_response(),
      Err(_) => (StatusCode::INTERNAL_SERVER_ERROR).into_response(),
    }
  }
}

impl<T, S: Send + Sync> FromRequest<S> for Xml<T>
where
  T: for<'de> Deserialize<'de>,
{
  type Rejection = XmlRejection;

  async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
    let bytes = Bytes::from_request(req, state).await?;
    Ok(Xml::from_slice(&bytes)?)
  }
}

impl<T, S: Send + Sync> OptionalFromRequest<S> for Xml<T>
where
  T: for<'de> Deserialize<'de>,
{
  type Rejection = XmlRejection;

  async fn from_request(req: Request, state: &S) -> Result<Option<Self>, Self::Rejection> {
    let bytes = Bytes::from_request(req, state).await?;
    match Xml::from_slice(&bytes) {
      Ok(xml) => Ok(Some(xml)),
      Err(_) => Ok(None),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[derive(Debug, Serialize, Deserialize, PartialEq)]
  struct Sample {
    name: String,
    value: i32,
  }

  #[test]
  fn test_xml_roundtrip() {
    let original = Sample {
      name: "hello".into(),
      value: 42,
    };
    let bytes = Xml(&original).to_slice().unwrap();

    // Serializing then deserializing must reproduce the original value.
    let Xml(parsed): Xml<Sample> = Xml::from_slice(&bytes).unwrap();
    assert_eq!(parsed, original);
  }

  #[test]
  fn test_xml_from_invalid_slice() {
    let result: Result<Xml<Sample>, _> = Xml::from_slice(b"not valid xml <<<");
    assert!(result.is_err());
  }

  #[test]
  fn test_xml_into_response_sets_content_type() {
    let response = Xml(Sample {
      name: "a".into(),
      value: 1,
    })
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
      response.headers().get(CONTENT_TYPE).unwrap(),
      "application/xml"
    );
  }

  #[test]
  fn test_rejection_into_response_is_bad_request() {
    let err = serde_xml_rs::from_str::<Sample>("<bad>").unwrap_err();
    let rejection = XmlRejection::InvalidXml(err);
    assert_eq!(rejection.into_response().status(), StatusCode::BAD_REQUEST);
  }

  fn xml_request(body: &'static [u8]) -> Request {
    Request::builder()
      .header(CONTENT_TYPE, "application/xml")
      .body(axum::body::Body::from(body))
      .unwrap()
  }

  #[tokio::test]
  async fn test_from_request_parses_valid_xml() {
    let body = b"<Sample><name>hi</name><value>5</value></Sample>";
    let Xml(parsed): Xml<Sample> =
      <Xml<Sample> as FromRequest<()>>::from_request(xml_request(body), &())
        .await
        .unwrap();
    assert_eq!(parsed.value, 5);
  }

  #[tokio::test]
  async fn test_from_request_rejects_invalid_xml() {
    let res = <Xml<Sample> as FromRequest<()>>::from_request(xml_request(b"<bad"), &()).await;
    assert!(res.is_err());
  }

  #[tokio::test]
  async fn test_optional_from_request_yields_none_on_invalid() {
    // The optional extractor swallows parse errors and returns None.
    let res: Option<Xml<Sample>> =
      <Xml<Sample> as OptionalFromRequest<()>>::from_request(xml_request(b"<bad"), &())
        .await
        .unwrap();
    assert!(res.is_none());

    let body = b"<Sample><name>x</name><value>1</value></Sample>";
    let res: Option<Xml<Sample>> =
      <Xml<Sample> as OptionalFromRequest<()>>::from_request(xml_request(body), &())
        .await
        .unwrap();
    assert!(res.is_some());
  }
}
