use eyre::ContextCompat;
use sea_orm::{ActiveValue::Set, prelude::*};

use crate::{db::entities::key, error::Result};

pub struct KeyTable<'db> {
  db: &'db DatabaseConnection,
}

impl<'db> KeyTable<'db> {
  pub fn new(db: &'db DatabaseConnection) -> Self {
    Self { db }
  }

  pub async fn get_key_by_name(&self, name: String) -> Result<key::Model> {
    let res = key::Entity::find()
      .filter(key::Column::Name.eq(&name))
      .one(self.db)
      .await?;

    Ok(res.context(format!("Key with name {} not found", name))?)
  }

  pub async fn create_key(&self, name: String, key: String, id: Uuid) -> Result<()> {
    let model = key::ActiveModel {
      name: Set(name),
      private_key: Set(key),
      id: Set(id),
    };

    model.insert(self.db).await?;

    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::config::DBConfig;
  use crate::db::init::connect_db;
  use crate::db::migrations::Migrator;
  use sea_orm_migration::MigratorTrait;

  #[tokio::test]
  async fn test_key_table() {
    let db_config = DBConfig::default();
    let conn = connect_db(&db_config, "sqlite::memory:").await;
    Migrator::up(&*conn, None).await.unwrap();

    let table = KeyTable::new(&conn);
    let id = Uuid::new_v4();
    table
      .create_key("test".into(), "private".into(), id)
      .await
      .unwrap();

    let key = table.get_key_by_name("test".into()).await.unwrap();
    assert_eq!(key.id, id);
    assert_eq!(key.private_key, "private");
  }
}
