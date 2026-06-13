use axum::{RequestPartsExt, extract::Query};
use axum_extra::{
  TypedHeader,
  extract::CookieJar,
  headers::{
    Authorization,
    authorization::{Basic, Bearer},
  },
};
use http::request::Parts;
use serde::Deserialize;

use crate::{bail, error::Result};

#[derive(Deserialize)]
struct Token {
  token: String,
}

pub async fn jwt_from_request(req: &mut Parts, token_name: &str) -> Result<String> {
  let bearer = req
    .extract::<TypedHeader<Authorization<Bearer>>>()
    .await
    .ok()
    .map(|TypedHeader(Authorization(bearer))| bearer.token().to_string());

  let token = match bearer {
    Some(token) => token,
    None => match req
      .extract::<CookieJar>()
      .await
      .ok()
      .and_then(|jar| jar.get(token_name).map(|cookie| cookie.value().to_string()))
    {
      Some(token) => token,
      None => match req.extract::<Query<Token>>().await {
        Ok(Query(token)) => token.token,
        Err(_) => {
          let Some(TypedHeader(Authorization(basic))) = req
            .extract::<TypedHeader<Authorization<Basic>>>()
            .await
            .ok()
          else {
            bail!("JWT token not found in Authorization header, cookies, or query parameters");
          };

          basic.password().to_string()
        }
      },
    },
  };

  Ok(token)
}

#[cfg(test)]
mod tests {
  use super::*;
  use axum::body::Body;
  use base64::Engine;

  fn parts_from(builder: http::request::Builder) -> Parts {
    builder.body(Body::empty()).unwrap().into_parts().0
  }

  #[tokio::test]
  async fn test_token_from_bearer_header() {
    let mut parts = parts_from(
      http::Request::builder()
        .uri("/")
        .header("authorization", "Bearer abc123"),
    );
    let token = jwt_from_request(&mut parts, "centaurus_jwt").await.unwrap();
    assert_eq!(token, "abc123");
  }

  #[tokio::test]
  async fn test_token_from_cookie() {
    let mut parts = parts_from(
      http::Request::builder()
        .uri("/")
        .header("cookie", "centaurus_jwt=cookie_token"),
    );
    let token = jwt_from_request(&mut parts, "centaurus_jwt").await.unwrap();
    assert_eq!(token, "cookie_token");
  }

  #[tokio::test]
  async fn test_token_from_query() {
    let mut parts = parts_from(http::Request::builder().uri("/path?token=query_token"));
    let token = jwt_from_request(&mut parts, "centaurus_jwt").await.unwrap();
    assert_eq!(token, "query_token");
  }

  #[tokio::test]
  async fn test_token_from_basic_auth_password() {
    let creds = base64::engine::general_purpose::STANDARD.encode("user:basic_token");
    let mut parts = parts_from(
      http::Request::builder()
        .uri("/")
        .header("authorization", format!("Basic {creds}")),
    );
    let token = jwt_from_request(&mut parts, "centaurus_jwt").await.unwrap();
    assert_eq!(token, "basic_token");
  }

  #[tokio::test]
  async fn test_bearer_takes_precedence_over_cookie() {
    // When multiple sources are present, the Authorization bearer wins.
    let mut parts = parts_from(
      http::Request::builder()
        .uri("/?token=query_token")
        .header("authorization", "Bearer bearer_token")
        .header("cookie", "centaurus_jwt=cookie_token"),
    );
    let token = jwt_from_request(&mut parts, "centaurus_jwt").await.unwrap();
    assert_eq!(token, "bearer_token");
  }

  #[tokio::test]
  async fn test_missing_token_errors() {
    let mut parts = parts_from(http::Request::builder().uri("/"));
    assert!(jwt_from_request(&mut parts, "centaurus_jwt").await.is_err());
  }

  #[tokio::test]
  async fn test_cookie_name_must_match() {
    // A cookie under a different name is not picked up.
    let mut parts = parts_from(
      http::Request::builder()
        .uri("/")
        .header("cookie", "other=value"),
    );
    assert!(jwt_from_request(&mut parts, "centaurus_jwt").await.is_err());
  }
}
