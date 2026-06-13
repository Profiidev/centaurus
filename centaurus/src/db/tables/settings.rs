use sea_orm::{IntoActiveModel, Set, prelude::*};

use crate::{
  db::{entities::settings, settings::Settings},
  error::Result,
};

pub struct SettingsTable<'db> {
  db: &'db DatabaseConnection,
}

impl<'db> SettingsTable<'db> {
  pub fn new(db: &'db DatabaseConnection) -> Self {
    Self { db }
  }

  pub async fn get_settings<S: Settings>(&self) -> Result<S> {
    let res = settings::Entity::find_by_id(S::id()).one(self.db).await?;
    let Some(model) = res else {
      return Ok(S::default());
    };

    Ok(serde_json::from_str(&model.content)?)
  }

  pub async fn save_settings<S: Settings>(&self, settings: &S) -> Result<()> {
    let content = serde_json::to_string(settings)?;

    match settings::Entity::find_by_id(S::id()).one(self.db).await? {
      Some(m) => {
        let mut am = m.into_active_model();
        am.content = Set(content);
        am.update(self.db).await?;
      }
      None => {
        let model = settings::Model {
          id: S::id(),
          content,
        };

        model.into_active_model().insert(self.db).await?;
      }
    };

    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::config::DBConfig;
  use crate::db::init::connect_db;
  use crate::db::migrations::Migrator;
  use schemars::JsonSchema;
  use sea_orm_migration::MigratorTrait;
  use serde::{Deserialize, Serialize};

  #[derive(Serialize, Deserialize, Default, PartialEq, Debug, JsonSchema)]
  struct TestSettings {
    val: String,
  }

  impl Settings for TestSettings {
    fn id() -> i32 {
      99
    }
  }

  #[tokio::test]
  async fn test_settings_table() {
    let db_config = DBConfig::default();
    let conn = connect_db(&db_config, "sqlite::memory:").await;
    Migrator::up(&*conn, None).await.unwrap();

    let table = SettingsTable::new(&conn);
    let s = TestSettings { val: "test".into() };
    table.save_settings(&s).await.unwrap();

    let s2: TestSettings = table.get_settings().await.unwrap();
    assert_eq!(s, s2);
  }

  #[tokio::test]
  async fn test_settings_default_when_absent() {
    let db_config = DBConfig::default();
    let conn = connect_db(&db_config, "sqlite::memory:").await;
    Migrator::up(&*conn, None).await.unwrap();

    let table = SettingsTable::new(&conn);
    // Reading settings that were never saved yields the type's default.
    let s: TestSettings = table.get_settings().await.unwrap();
    assert_eq!(s, TestSettings::default());
  }

  #[tokio::test]
  async fn test_settings_overwrite() {
    let db_config = DBConfig::default();
    let conn = connect_db(&db_config, "sqlite::memory:").await;
    Migrator::up(&*conn, None).await.unwrap();

    let table = SettingsTable::new(&conn);
    table
      .save_settings(&TestSettings {
        val: "first".into(),
      })
      .await
      .unwrap();
    table
      .save_settings(&TestSettings {
        val: "second".into(),
      })
      .await
      .unwrap();

    // Saving the same id twice updates in place rather than duplicating.
    let s: TestSettings = table.get_settings().await.unwrap();
    assert_eq!(s.val, "second");
  }
}
