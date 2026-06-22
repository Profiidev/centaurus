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

#[async_trait::async_trait]
pub trait Auth: Send + Sync + 'static {
  async fn check(
    &self,
    db: &Connection,
    parts: &mut Parts,
    token: &str,
    claims: &JwtClaims,
  ) -> Result<(), ErrorReport>;
}

impl<S: Sync, P: Permission> FromRequestParts<S> for JwtAuth<P> {
  type Rejection = ErrorReport;

  async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
    let token = jwt_from_request(parts, JWT_COOKIE_NAME).await?;

    let db = parts.extract_state::<Connection>().await;
    let state = parts.extract_state::<JwtState>().await;

    let Ok(claims) = state.validate_token(&token) else {
      tracing::error!("invalid token claims for token: {}", token);
      bail!(UNAUTHORIZED, "invalid token");
    };

    state.auth.check(&db, parts, &token, &claims).await?;
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

pub struct StatelessAuth;

#[async_trait::async_trait]
impl Auth for StatelessAuth {
  async fn check(
    &self,
    db: &Connection,
    _parts: &mut Parts,
    token: &str,
    _claims: &JwtClaims,
  ) -> Result<(), ErrorReport> {
    let Ok(valid) = db.invalid_jwt().is_token_valid(token).await else {
      bail!("failed to validate jwt");
    };
    if !valid {
      bail!(UNAUTHORIZED, "token is invalidated");
    }

    Ok(())
  }
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
  use crate::backend::auth::settings::AuthConfig;
  use crate::db::config::DBConfig;
  use crate::db::init::connect_db;
  use crate::db::migrations::Migrator;
  use axum::body::Body;
  use axum::extract::FromRequestParts;
  use http::StatusCode;
  use sea_orm_migration::MigratorTrait;
  use std::sync::Arc;
  use std::sync::Mutex;
  use std::sync::atomic::{AtomicBool, Ordering};

  async fn db() -> Connection {
    let conn = connect_db(&DBConfig::default(), "sqlite::memory:").await;
    Migrator::up(&*conn, None).await.unwrap();
    conn
  }

  async fn create_user(conn: &Connection) -> Uuid {
    conn
      .user()
      .create_user(
        "u".into(),
        "u@x.com".into(),
        "h".into(),
        "s".into(),
        false,
        None,
      )
      .await
      .unwrap()
  }

  fn parts_with_token(db: Connection, state: JwtState, token: &str) -> Parts {
    http::Request::builder()
      .uri("/")
      .header("authorization", format!("Bearer {token}"))
      .extension(db)
      .extension(state)
      .body(Body::empty())
      .unwrap()
      .into_parts()
      .0
  }

  fn empty_parts() -> Parts {
    http::Request::builder()
      .body(Body::empty())
      .unwrap()
      .into_parts()
      .0
  }

  // ----- StatelessAuth -----

  #[tokio::test]
  async fn test_stateless_auth_accepts_non_invalidated_token() {
    let conn = db().await;
    let state = JwtState::init(&AuthConfig::default(), &conn).await;
    let token = state.create_raw_token(Uuid::now_v7()).unwrap();
    let claims = state.validate_token(&token).unwrap();

    let mut parts = empty_parts();
    assert!(
      StatelessAuth
        .check(&conn, &mut parts, &token, &claims)
        .await
        .is_ok()
    );
  }

  #[tokio::test]
  async fn test_stateless_auth_rejects_invalidated_token() {
    use std::sync::atomic::AtomicI32;

    let conn = db().await;
    let state = JwtState::init(&AuthConfig::default(), &conn).await;
    let token = state.create_raw_token(Uuid::now_v7()).unwrap();
    let claims = state.validate_token(&token).unwrap();

    conn
      .invalid_jwt()
      .invalidate_jwt(
        token.clone(),
        chrono::Utc::now() + chrono::Duration::seconds(3600),
        Arc::new(AtomicI32::new(0)),
      )
      .await
      .unwrap();

    let mut parts = empty_parts();
    let err = StatelessAuth
      .check(&conn, &mut parts, &token, &claims)
      .await
      .unwrap_err();
    assert_eq!(err.status, StatusCode::UNAUTHORIZED);
  }

  // ----- Custom Auth implementations used by the generic path -----

  struct RecordingAuth {
    allow: bool,
    called: Arc<AtomicBool>,
    seen_token: Arc<Mutex<Option<String>>>,
    seen_sub: Arc<Mutex<Option<Uuid>>>,
  }

  #[async_trait::async_trait]
  impl Auth for RecordingAuth {
    async fn check(
      &self,
      _db: &Connection,
      _parts: &mut Parts,
      token: &str,
      claims: &JwtClaims,
    ) -> Result<(), ErrorReport> {
      self.called.store(true, Ordering::Relaxed);
      *self.seen_token.lock().unwrap() = Some(token.to_string());
      *self.seen_sub.lock().unwrap() = Some(claims.sub);
      if self.allow {
        Ok(())
      } else {
        crate::bail!(UNAUTHORIZED, "rejected by custom auth");
      }
    }
  }

  // ----- Full JwtAuth::from_request_parts flow -----

  #[tokio::test]
  async fn test_from_request_parts_succeeds_for_valid_token() {
    let conn = db().await;
    let uid = create_user(&conn).await;
    let state = JwtState::init(&AuthConfig::default(), &conn).await;
    let token = state.create_raw_token(uid).unwrap();

    let mut parts = parts_with_token(conn.clone(), state, &token);
    let Ok(auth) =
      <JwtAuth<NoPerm> as FromRequestParts<()>>::from_request_parts(&mut parts, &()).await
    else {
      panic!("expected valid token to authenticate");
    };
    assert_eq!(auth.user_id, uid);
  }

  #[tokio::test]
  async fn test_from_request_parts_rejects_garbage_token() {
    let conn = db().await;
    let state = JwtState::init(&AuthConfig::default(), &conn).await;

    let mut parts = parts_with_token(conn.clone(), state, "not.a.jwt");
    let Err(err) =
      <JwtAuth<NoPerm> as FromRequestParts<()>>::from_request_parts(&mut parts, &()).await
    else {
      panic!("expected garbage token to be rejected");
    };
    assert_eq!(err.status, StatusCode::UNAUTHORIZED);
  }

  #[tokio::test]
  async fn test_from_request_parts_rejects_missing_token() {
    let conn = db().await;
    let state = JwtState::init(&AuthConfig::default(), &conn).await;
    // No Authorization header / cookie / query token at all.
    let mut parts = http::Request::builder()
      .uri("/")
      .extension(conn.clone())
      .extension(state)
      .body(Body::empty())
      .unwrap()
      .into_parts()
      .0;
    assert!(
      <JwtAuth<NoPerm> as FromRequestParts<()>>::from_request_parts(&mut parts, &())
        .await
        .is_err()
    );
  }

  #[tokio::test]
  async fn test_from_request_parts_rejects_invalidated_token_via_stateless_auth() {
    use std::sync::atomic::AtomicI32;

    let conn = db().await;
    let uid = create_user(&conn).await;
    let state = JwtState::init(&AuthConfig::default(), &conn).await;
    let token = state.create_raw_token(uid).unwrap();

    conn
      .invalid_jwt()
      .invalidate_jwt(
        token.clone(),
        chrono::Utc::now() + chrono::Duration::seconds(3600),
        Arc::new(AtomicI32::new(0)),
      )
      .await
      .unwrap();

    let mut parts = parts_with_token(conn.clone(), state, &token);
    let Err(err) =
      <JwtAuth<NoPerm> as FromRequestParts<()>>::from_request_parts(&mut parts, &()).await
    else {
      panic!("expected invalidated token to be rejected");
    };
    assert_eq!(err.status, StatusCode::UNAUTHORIZED);
  }

  #[tokio::test]
  async fn test_from_request_parts_fails_permission_check() {
    let conn = db().await;
    let uid = create_user(&conn).await;
    let state = JwtState::init(&AuthConfig::default(), &conn).await;
    let token = state.create_raw_token(uid).unwrap();

    // User exists and token is valid, but lacks the required permission.
    let mut parts = parts_with_token(conn.clone(), state, &token);
    let Err(err) =
      <JwtAuth<UserEdit> as FromRequestParts<()>>::from_request_parts(&mut parts, &()).await
    else {
      panic!("expected missing permission to be rejected");
    };
    assert_eq!(err.status, StatusCode::FORBIDDEN);
  }

  #[tokio::test]
  async fn test_from_request_parts_passes_permission_check_when_granted() {
    let conn = db().await;
    let uid = create_user(&conn).await;
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

    let state = JwtState::init(&AuthConfig::default(), &conn).await;
    let token = state.create_raw_token(uid).unwrap();

    let mut parts = parts_with_token(conn.clone(), state, &token);
    let Ok(auth) =
      <JwtAuth<UserEdit> as FromRequestParts<()>>::from_request_parts(&mut parts, &()).await
    else {
      panic!("expected granted permission to authenticate");
    };
    assert_eq!(auth.user_id, uid);
  }

  #[tokio::test]
  async fn test_from_request_parts_invokes_custom_auth_with_token_and_claims() {
    let conn = db().await;
    let uid = create_user(&conn).await;
    let called = Arc::new(AtomicBool::new(false));
    let seen_token = Arc::new(Mutex::new(None));
    let seen_sub = Arc::new(Mutex::new(None));
    let auth = RecordingAuth {
      allow: true,
      called: called.clone(),
      seen_token: seen_token.clone(),
      seen_sub: seen_sub.clone(),
    };
    let state = JwtState::init_with_auth(&AuthConfig::default(), &conn, auth).await;
    let token = state.create_raw_token(uid).unwrap();

    let mut parts = parts_with_token(conn.clone(), state, &token);
    let Ok(result) =
      <JwtAuth<NoPerm> as FromRequestParts<()>>::from_request_parts(&mut parts, &()).await
    else {
      panic!("expected custom auth to allow the request");
    };

    assert_eq!(result.user_id, uid);
    assert!(called.load(Ordering::Relaxed));
    assert_eq!(seen_token.lock().unwrap().as_deref(), Some(token.as_str()));
    assert_eq!(*seen_sub.lock().unwrap(), Some(uid));
  }

  #[tokio::test]
  async fn test_from_request_parts_rejected_by_custom_auth() {
    let conn = db().await;
    let uid = create_user(&conn).await;
    let auth = RecordingAuth {
      allow: false,
      called: Arc::new(AtomicBool::new(false)),
      seen_token: Arc::new(Mutex::new(None)),
      seen_sub: Arc::new(Mutex::new(None)),
    };
    let state = JwtState::init_with_auth(&AuthConfig::default(), &conn, auth).await;
    let token = state.create_raw_token(uid).unwrap();

    let mut parts = parts_with_token(conn.clone(), state, &token);
    let Err(err) =
      <JwtAuth<NoPerm> as FromRequestParts<()>>::from_request_parts(&mut parts, &()).await
    else {
      panic!("expected custom auth to reject the request");
    };
    assert_eq!(err.status, StatusCode::UNAUTHORIZED);
  }

  // ----- OptionalFromRequestParts -----

  #[tokio::test]
  async fn test_optional_from_request_parts_some_on_success() {
    let conn = db().await;
    let uid = create_user(&conn).await;
    let state = JwtState::init(&AuthConfig::default(), &conn).await;
    let token = state.create_raw_token(uid).unwrap();

    let mut parts = parts_with_token(conn.clone(), state, &token);
    let Ok(auth) =
      <JwtAuth<NoPerm> as OptionalFromRequestParts<()>>::from_request_parts(&mut parts, &()).await
    else {
      panic!("optional extractor must not error on valid token");
    };
    assert_eq!(auth.unwrap().user_id, uid);
  }

  #[tokio::test]
  async fn test_optional_from_request_parts_none_on_failure() {
    let conn = db().await;
    let state = JwtState::init(&AuthConfig::default(), &conn).await;

    let mut parts = parts_with_token(conn.clone(), state, "not.a.jwt");
    let Ok(auth) =
      <JwtAuth<NoPerm> as OptionalFromRequestParts<()>>::from_request_parts(&mut parts, &()).await
    else {
      panic!("optional extractor must swallow errors into None");
    };
    assert!(auth.is_none());
  }

  #[allow(deprecated)]
  #[tokio::test]
  async fn test_check_user_no_permission_required() {
    let conn = db().await;
    let uid = conn
      .user()
      .create_user(
        "u".into(),
        "u@x.com".into(),
        "h".into(),
        "s".into(),
        false,
        None,
      )
      .await
      .unwrap();

    // NoPerm only requires that the user exists.
    assert!(check_user::<NoPerm>(&conn, uid).await.is_ok());
    assert!(check_user::<NoPerm>(&conn, Uuid::now_v7()).await.is_err());
  }

  #[allow(deprecated)]
  #[tokio::test]
  async fn test_check_user_with_permission() {
    let conn = db().await;
    let uid = conn
      .user()
      .create_user(
        "u".into(),
        "u@x.com".into(),
        "h".into(),
        "s".into(),
        false,
        None,
      )
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
