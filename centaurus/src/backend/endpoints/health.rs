#[cfg(any(feature = "db", feature = "storage"))]
use std::time::Duration;

#[cfg(any(feature = "db", feature = "storage"))]
use axum::Extension;
use std::sync::Arc;

use axum::{Extension as Ext, Router, routing::get};
use http::StatusCode;

/// App-specific readiness, run by `/ready` after the built-in checks.
/// Register it as an extension: `router.layer(Extension(ReadyCheck::new(|| Ok(()))))`.
/// The error text is the 503 response body.
#[derive(Clone)]
pub struct ReadyCheck(Arc<dyn Fn() -> Result<(), &'static str> + Send + Sync>);

impl ReadyCheck {
  pub fn new(check: impl Fn() -> Result<(), &'static str> + Send + Sync + 'static) -> Self {
    Self(Arc::new(check))
  }
}

/// Upper bound for each dependency check in `/ready`.
#[cfg(any(feature = "db", feature = "storage"))]
const READY_TIMEOUT: Duration = Duration::from_secs(2);

/// `/health` is a liveness check (process answers), `/ready` a readiness check
/// (dependencies reachable). Only point a k8s readinessProbe at `/ready`: a
/// failing liveness probe restarts the pod, which does not fix a DB outage.
pub fn router() -> Router {
  Router::new()
    .route("/health", get(health_check))
    .route("/ready", get(ready_check))
}

async fn health_check() -> &'static str {
  "OK"
}

/// Checks only the dependencies the app registered as extensions.
async fn ready_check(
  #[cfg(feature = "db")] db: Option<Extension<crate::db::init::Connection>>,
  #[cfg(feature = "storage")] storage: Option<Extension<crate::storage::FileStorage>>,
  app_check: Option<Ext<ReadyCheck>>,
) -> (StatusCode, &'static str) {
  #[cfg(feature = "db")]
  if let Some(Extension(db)) = db {
    let res = check(db.ping()).await;
    if let Err(e) = res {
      tracing::warn!("Readiness check failed for database: {e}");
      return (StatusCode::SERVICE_UNAVAILABLE, "database unavailable");
    }
  }

  #[cfg(feature = "storage")]
  if let Some(Extension(storage)) = storage {
    // A missing object still proves the store answers; only transport errors fail.
    let path = crate::storage::StoragePath::from("centaurus-ready-check");
    if let Err(e) = check(storage.exists(&path)).await {
      tracing::warn!("Readiness check failed for storage: {e}");
      return (StatusCode::SERVICE_UNAVAILABLE, "storage unavailable");
    }
  }

  if let Some(Ext(ReadyCheck(app_check))) = app_check
    && let Err(e) = app_check()
  {
    return (StatusCode::SERVICE_UNAVAILABLE, e);
  }

  (StatusCode::OK, "OK")
}

#[cfg(any(feature = "db", feature = "storage"))]
async fn check<T, E: std::fmt::Display>(
  fut: impl Future<Output = Result<T, E>>,
) -> Result<(), String> {
  match tokio::time::timeout(READY_TIMEOUT, fut).await {
    Ok(Ok(_)) => Ok(()),
    Ok(Err(e)) => Err(e.to_string()),
    Err(_) => Err(format!("timed out after {READY_TIMEOUT:?}")),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use axum::body::Body;
  use http::{Request, StatusCode};
  use tower::ServiceExt;

  #[tokio::test]
  async fn test_health_check_returns_ok() {
    let app = router();
    let response = app
      .oneshot(
        Request::builder()
          .uri("/health")
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
      .await
      .unwrap();
    assert_eq!(&body[..], b"OK");
  }

  async fn get_ready(app: Router) -> (StatusCode, String) {
    let response = app
      .oneshot(
        Request::builder()
          .uri("/ready")
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
      .await
      .unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
  }

  #[tokio::test]
  async fn test_ready_without_dependencies_is_ok() {
    assert_eq!(get_ready(router()).await, (StatusCode::OK, "OK".into()));
  }

  #[tokio::test]
  async fn test_ready_runs_app_check() {
    let ok = router().layer(Ext(ReadyCheck::new(|| Ok(()))));
    assert_eq!(get_ready(ok).await, (StatusCode::OK, "OK".into()));
    let failing = router().layer(Ext(ReadyCheck::new(|| Err("not ready"))));
    assert_eq!(
      get_ready(failing).await,
      (StatusCode::SERVICE_UNAVAILABLE, "not ready".into())
    );
  }

  #[cfg(feature = "db")]
  #[tokio::test]
  async fn test_ready_with_db_is_ok() {
    use crate::db::{config::DBConfig, init::connect_db};
    let conn = connect_db(&DBConfig::default(), "sqlite::memory:").await;
    let app = router().layer(axum::Extension(conn));
    assert_eq!(get_ready(app).await, (StatusCode::OK, "OK".into()));
  }

  #[cfg(feature = "db")]
  #[tokio::test]
  async fn test_ready_with_closed_db_is_unavailable() {
    use crate::db::{config::DBConfig, init::connect_db};
    let conn = connect_db(&DBConfig::default(), "sqlite::memory:").await;
    conn.close_by_ref().await.unwrap();
    let app = router().layer(axum::Extension(conn));
    assert_eq!(
      get_ready(app).await,
      (
        StatusCode::SERVICE_UNAVAILABLE,
        "database unavailable".into()
      )
    );
  }

  #[cfg(feature = "storage")]
  #[tokio::test]
  async fn test_ready_with_storage_is_ok() {
    use crate::storage::{FileStorage, StorageConfig};
    let dir = tempfile::tempdir().unwrap();
    let storage = FileStorage::init(&StorageConfig {
      storage_path: dir.path().to_str().unwrap().to_string(),
      ..Default::default()
    })
    .await
    .unwrap();
    let app = router().layer(axum::Extension(storage));
    assert_eq!(get_ready(app).await, (StatusCode::OK, "OK".into()));
  }

  #[tokio::test]
  async fn test_unknown_route_is_not_found() {
    let response = router()
      .oneshot(
        Request::builder()
          .uri("/missing")
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
  }
}
