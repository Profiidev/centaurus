use crate::db::{
  init::Connection,
  tables::{invalid_jwt::InvalidJwtTable, key::KeyTable, settings::SettingsTable},
};

pub mod invalid_jwt;
pub mod key;
pub mod settings;

pub trait ConnectionExt {
  fn key(&self) -> KeyTable<'_>;
  fn invalid_jwt(&self) -> InvalidJwtTable<'_>;
  fn settings(&self) -> SettingsTable<'_>;
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
}
