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
