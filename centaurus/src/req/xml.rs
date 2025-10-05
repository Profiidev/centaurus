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
