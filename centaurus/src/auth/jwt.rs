use axum::{RequestPartsExt, extract::Query};
use axum_extra::{
  TypedHeader,
  extract::CookieJar,
  headers::{Authorization, authorization::Bearer},
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
      None => {
        let Some(Query(token)) = req.extract::<Query<Token>>().await.ok() else {
          bail!("JWT token not found in Authorization header, cookies, or query parameters");
        };

        token.token
      }
    },
  };

  Ok(token)
}
