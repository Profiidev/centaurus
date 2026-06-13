use sea_orm::{ActiveValue::Set, IntoActiveModel, prelude::*};

use crate::{db::entities::setup, error::Result};

const SETUP_ID: i32 = 1;

pub struct SetupTable<'db> {
  db: &'db DatabaseConnection,
}

impl<'db> SetupTable<'db> {
  pub fn new(db: &'db DatabaseConnection) -> Self {
    Self { db }
  }

  pub async fn is_setup(&self) -> Result<bool> {
    let res = setup::Entity::find_by_id(SETUP_ID).one(self.db).await?;
    Ok(res.is_some_and(|s| s.completed))
  }

  async fn get_setup(&self) -> Result<setup::Model> {
    let res = setup::Entity::find_by_id(SETUP_ID).one(self.db).await?;

    if let Some(model) = res {
      Ok(model)
    } else {
      Ok(
        setup::ActiveModel {
          id: Set(SETUP_ID),
          admin_group_created: Set(None),
          completed: Set(false),
        }
        .insert(self.db)
        .await?,
      )
    }
  }

  pub async fn mark_completed(&self) -> Result<()> {
    let mut model = self.get_setup().await?.into_active_model();
    model.completed = Set(true);

    model.update(self.db).await?;

    Ok(())
  }

  pub async fn set_admin_group_created(&self, group_id: Uuid) -> Result<()> {
    let mut model = self.get_setup().await?.into_active_model();
    model.admin_group_created = Set(Some(group_id));

    model.update(self.db).await?;

    Ok(())
  }

  pub async fn get_admin_group_id(&self) -> Result<Option<Uuid>> {
    let model = self.get_setup().await?;
    Ok(model.admin_group_created)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::config::DBConfig;
  use crate::db::init::{Connection, connect_db};
  use crate::db::migrations::Migrator;
  use sea_orm_migration::MigratorTrait;

  async fn setup() -> Connection {
    let db_config = DBConfig::default();
    let conn = connect_db(&db_config, "sqlite::memory:").await;
    Migrator::up(&*conn, None).await.unwrap();
    conn
  }

  #[tokio::test]
  async fn test_fresh_setup_state() {
    let conn = setup().await;
    let table = SetupTable::new(&conn);

    // A fresh database is not set up and has no admin group.
    assert!(!table.is_setup().await.unwrap());
    assert_eq!(table.get_admin_group_id().await.unwrap(), None);
  }

  #[tokio::test]
  async fn test_mark_completed() {
    let conn = setup().await;
    let table = SetupTable::new(&conn);

    table.mark_completed().await.unwrap();
    assert!(table.is_setup().await.unwrap());

    // Marking completed again is idempotent.
    table.mark_completed().await.unwrap();
    assert!(table.is_setup().await.unwrap());
  }

  #[tokio::test]
  async fn test_admin_group_persistence() {
    let conn = setup().await;
    let table = SetupTable::new(&conn);
    let group = Uuid::new_v4();

    table.set_admin_group_created(group).await.unwrap();
    assert_eq!(table.get_admin_group_id().await.unwrap(), Some(group));

    // The admin group can be reassigned, and is independent of completion.
    let group2 = Uuid::new_v4();
    table.set_admin_group_created(group2).await.unwrap();
    assert_eq!(table.get_admin_group_id().await.unwrap(), Some(group2));
    assert!(!table.is_setup().await.unwrap());
  }
}
