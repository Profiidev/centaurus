use std::marker::PhantomData;

use aide::OperationIo;
use axum::extract::{FromRequestParts, OptionalFromRequestParts};
use http::request::Parts;
use uuid::Uuid;

use crate::{
  backend::auth::{
    jwt::jwt_from_request,
    jwt_auth::{JWT_COOKIE_NAME, JwtClaims, JwtState},
    permission::{NoPerm, Permission},
  },
  bail,
  db::{init::Connection, tables::ConnectionExt},
  error::ErrorReport,
  state::extract::StateExtractExt,
};

#[derive(Debug, OperationIo)]
pub struct JwtAuth<P: Permission = NoPerm> {
  pub user_id: Uuid,
  pub exp: i64,
  _perm: PhantomData<P>,
}

impl<S: Sync, P: Permission> FromRequestParts<S> for JwtAuth<P> {
  type Rejection = ErrorReport;

  async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
    let token = jwt_from_request(parts, JWT_COOKIE_NAME).await?;

    let db = parts.extract_state::<Connection>().await;
    let claims = check_jwt(&db, parts, token).await?;
    check_user::<P>(&db, claims.sub).await?;

    Ok(JwtAuth {
      user_id: claims.sub,
      exp: claims.exp,
      _perm: PhantomData,
    })
  }
}

impl<S: Sync, P: Permission> OptionalFromRequestParts<S> for JwtAuth<P> {
  type Rejection = ErrorReport;

  async fn from_request_parts(
    parts: &mut Parts,
    state: &S,
  ) -> Result<Option<Self>, Self::Rejection> {
    match <Self as FromRequestParts<S>>::from_request_parts(parts, state).await {
      Ok(auth) => Ok(Some(auth)),
      Err(_) => Ok(None),
    }
  }
}

pub async fn check_jwt(
  db: &Connection,
  parts: &mut Parts,
  token: String,
) -> Result<JwtClaims, ErrorReport> {
  let state = parts.extract_state::<JwtState>().await;

  let Ok(valid) = db.invalid_jwt().is_token_valid(&token).await else {
    bail!("failed to validate jwt");
  };
  if !valid {
    bail!(UNAUTHORIZED, "token is invalidated");
  }

  let Ok(claims) = state.validate_token(&token) else {
    tracing::error!("invalid token claims for token: {}", token);
    bail!(UNAUTHORIZED, "invalid token");
  };

  Ok(claims)
}

pub async fn check_user<P: Permission>(db: &Connection, user: Uuid) -> Result<(), ErrorReport> {
  // Empty permission means no permission required
  if !P::name().is_empty() {
    // This check automatically checks if the user exists, because if the user doesn't exist, they won't have any permissions
    if !db.group().user_hash_permissions(user, P::name()).await? {
      bail!(FORBIDDEN, "insufficient permissions");
    }
  } else if db.user().get_user_by_id(user).await.is_err() {
    // If no permission is required, just check if the user exists
    bail!(FORBIDDEN, "user does not exist");
  }

  Ok(())
}
