use std::marker::PhantomData;

use aide::OperationIo;
use axum::extract::{FromRequestParts, OptionalFromRequestParts};
use http::request::Parts;
use uuid::Uuid;

use crate::{
  backend::{
    auth::{
      jwt::jwt_from_request,
      jwt_state::{JWT_COOKIE_NAME, JwtClaims, JwtState},
      permission::{NoPerm, Permission},
    },
    request::extract::StateExtractExt,
  },
  bail,
  db::{init::Connection, tables::ConnectionExt},
  error::ErrorReport,
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
    P::check(&db, claims.sub, parts).await?;

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

#[deprecated]
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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::backend::auth::permission::UserEdit;
  use crate::db::config::DBConfig;
  use crate::db::init::connect_db;
  use crate::db::migrations::Migrator;
  use sea_orm_migration::MigratorTrait;

  async fn db() -> Connection {
    let conn = connect_db(&DBConfig::default(), "sqlite::memory:").await;
    Migrator::up(&*conn, None).await.unwrap();
    conn
  }

  #[allow(deprecated)]
  #[tokio::test]
  async fn test_check_user_no_permission_required() {
    let conn = db().await;
    let uid = conn
      .user()
      .create_user("u".into(), "u@x.com".into(), "h".into(), "s".into(), false)
      .await
      .unwrap();

    // NoPerm only requires that the user exists.
    assert!(check_user::<NoPerm>(&conn, uid).await.is_ok());
    assert!(check_user::<NoPerm>(&conn, Uuid::new_v4()).await.is_err());
  }

  #[allow(deprecated)]
  #[tokio::test]
  async fn test_check_user_with_permission() {
    let conn = db().await;
    let uid = conn
      .user()
      .create_user("u".into(), "u@x.com".into(), "h".into(), "s".into(), false)
      .await
      .unwrap();

    // Without the permission the check fails...
    assert!(check_user::<UserEdit>(&conn, uid).await.is_err());

    // ...and succeeds once granted via a group.
    let group = conn.group().create_group("g".into()).await.unwrap();
    conn
      .group()
      .add_permissions_to_group(group, vec!["user:edit".into()])
      .await
      .unwrap();
    conn
      .group()
      .add_users_to_group(group, vec![uid])
      .await
      .unwrap();
    assert!(check_user::<UserEdit>(&conn, uid).await.is_ok());
  }
}
