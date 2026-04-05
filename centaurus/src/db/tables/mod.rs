use crate::db::{
  init::Connection,
  tables::{
    group::GroupTable, invalid_jwt::InvalidJwtTable, key::KeyTable, settings::SettingsTable,
    user::UserTable,
  },
};

pub mod group;
pub mod invalid_jwt;
pub mod key;
pub mod settings;
pub mod setup;
pub mod user;

pub trait ConnectionExt {
  fn key(&self) -> KeyTable<'_>;
  fn invalid_jwt(&self) -> InvalidJwtTable<'_>;
  fn settings(&self) -> SettingsTable<'_>;
  fn user(&self) -> user::UserTable<'_>;
  fn group(&self) -> group::GroupTable<'_>;
  fn setup(&self) -> setup::SetupTable<'_>;
}

impl ConnectionExt for Connection {
  fn key(&self) -> KeyTable<'_> {
    KeyTable::new(self)
  }

  fn invalid_jwt(&self) -> InvalidJwtTable<'_> {
    InvalidJwtTable::new(self)
  }

  fn settings(&self) -> SettingsTable<'_> {
    SettingsTable::new(self)
  }

  fn user(&self) -> user::UserTable<'_> {
    UserTable::new(self)
  }

  fn group(&self) -> group::GroupTable<'_> {
    GroupTable::new(self)
  }

  fn setup(&self) -> setup::SetupTable<'_> {
    setup::SetupTable::new(self)
  }
}
