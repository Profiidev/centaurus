pub trait Permission {
  fn name() -> &'static str;
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
