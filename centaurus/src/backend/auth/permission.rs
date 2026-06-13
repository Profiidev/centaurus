use http::request::Parts;
use uuid::Uuid;

use crate::{
  bail,
  db::{init::Connection, tables::ConnectionExt},
  error::Result,
};

pub trait Permission {
  fn name() -> &'static str {
    ""
  }

  fn check(db: &Connection, user: Uuid, _parts: &Parts) -> impl Future<Output = Result<()>> + Send {
    async move {
      // Empty permission means no permission required
      if !Self::name().is_empty() {
        // This check automatically checks if the user exists, because if the user doesn't exist, they won't have any permissions
        if !db.group().user_hash_permissions(user, Self::name()).await? {
          bail!(FORBIDDEN, "insufficient permissions");
        }
      } else if db.user().get_user_by_id(user).await.is_err() {
        // If no permission is required, just check if the user exists
        bail!(FORBIDDEN, "user does not exist");
      }

      Ok(())
    }
  }
}

pub fn permissions() -> Vec<&'static str> {
  vec![
    SettingsView::name(),
    SettingsEdit::name(),
    GroupView::name(),
    GroupEdit::name(),
    UserView::name(),
    UserEdit::name(),
  ]
}

#[macro_export]
macro_rules! permission {
  ($type:ident, $name:literal) => {
    pub struct $type;

    impl $crate::backend::auth::permission::Permission for $type {
      fn name() -> &'static str {
        $name
      }
    }
  };
}

// No permissions required
permission!(NoPerm, "");

// Settings
permission!(SettingsView, "settings:view");
permission!(SettingsEdit, "settings:edit");

// Groups
permission!(GroupView, "group:view");
permission!(GroupEdit, "group:edit");

// Users
permission!(UserView, "user:view");
permission!(UserEdit, "user:edit");

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::config::DBConfig;
  use crate::db::init::{Connection, connect_db};
  use crate::db::migrations::Migrator;
  use crate::db::tables::ConnectionExt;
  use http::request::Parts;
  use sea_orm_migration::MigratorTrait;

  async fn db() -> Connection {
    let conn = connect_db(&DBConfig::default(), "sqlite::memory:").await;
    Migrator::up(&*conn, None).await.unwrap();
    conn
  }

  fn empty_parts() -> Parts {
    http::Request::builder().body(()).unwrap().into_parts().0
  }

  #[test]
  fn test_permissions_list_is_complete() {
    let perms = permissions();
    assert!(perms.contains(&"settings:view"));
    assert!(perms.contains(&"user:edit"));
    assert_eq!(perms.len(), 6);
    // NoPerm has an empty name.
    assert_eq!(NoPerm::name(), "");
  }

  #[tokio::test]
  async fn test_noperm_requires_existing_user() {
    let conn = db().await;
    let parts = empty_parts();
    let uid = conn
      .user()
      .create_user("u".into(), "u@x.com".into(), "h".into(), "s".into(), false)
      .await
      .unwrap();

    assert!(NoPerm::check(&conn, uid, &parts).await.is_ok());
    assert!(NoPerm::check(&conn, Uuid::new_v4(), &parts).await.is_err());
  }

  #[tokio::test]
  async fn test_named_permission_check() {
    let conn = db().await;
    let parts = empty_parts();
    let uid = conn
      .user()
      .create_user("u".into(), "u@x.com".into(), "h".into(), "s".into(), false)
      .await
      .unwrap();

    // Missing the permission ⇒ forbidden.
    assert!(UserView::check(&conn, uid, &parts).await.is_err());

    let group = conn.group().create_group("g".into()).await.unwrap();
    conn
      .group()
      .add_permissions_to_group(group, vec!["user:view".into()])
      .await
      .unwrap();
    conn
      .group()
      .add_users_to_group(group, vec![uid])
      .await
      .unwrap();
    assert!(UserView::check(&conn, uid, &parts).await.is_ok());
  }
}
