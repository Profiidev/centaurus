use std::sync::{
  Arc,
  atomic::{AtomicI32, Ordering},
};

use chrono::{DateTime, Utc};
use sea_orm::{ActiveValue::Set, prelude::*};
use tracing::instrument;

use crate::{db::entities::invalid_jwt, error::Result};

pub struct InvalidJwtTable<'db> {
  db: &'db DatabaseConnection,
}

impl<'db> InvalidJwtTable<'db> {
  pub fn new(db: &'db DatabaseConnection) -> Self {
    Self { db }
  }

  #[instrument(skip(self))]
  pub async fn invalidate_jwt(
    &self,
    token: String,
    exp: DateTime<Utc>,
    invalid_count: Arc<AtomicI32>,
  ) -> Result<()> {
    let model = invalid_jwt::ActiveModel {
      token: Set(token),
      exp: Set(exp.naive_utc()),
      id: Set(Uuid::new_v4()),
    };
    model.insert(self.db).await?;

    if invalid_count.load(Ordering::Relaxed) > 1000 {
      self.remove_expired().await?;
      invalid_count.store(0, Ordering::Relaxed);
    } else {
      invalid_count.fetch_add(1, Ordering::Relaxed);
    }

    Ok(())
  }

  #[instrument(skip(self))]
  pub async fn is_token_valid(&self, token: &str) -> Result<bool> {
    let res = invalid_jwt::Entity::find()
      .filter(invalid_jwt::Column::Token.eq(token))
      .one(self.db)
      .await?;

    Ok(res.is_none())
  }

  #[instrument(skip(self))]
  pub async fn remove_expired(&self) -> Result<()> {
    invalid_jwt::Entity::delete_many()
      .filter(invalid_jwt::Column::Exp.lt(Utc::now().naive_utc()))
      .exec(self.db)
      .await?;

    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::config::DBConfig;
  use crate::db::init::connect_db;
  use crate::db::migrations::Migrator;
  use chrono::Duration;
  use sea_orm_migration::MigratorTrait;

  #[tokio::test]
  async fn test_invalid_jwt_table() {
    let db_config = DBConfig::default();
    let conn = connect_db(&db_config, "sqlite::memory:").await;
    Migrator::up(&*conn, None).await.unwrap();

    let table = InvalidJwtTable::new(&conn);
    let count = Arc::new(AtomicI32::new(0));
    let token = "test_token".to_string();
    let exp = Utc::now() + Duration::seconds(3600);

    table
      .invalidate_jwt(token.clone(), exp, count.clone())
      .await
      .unwrap();
    assert!(!table.is_token_valid(&token).await.unwrap());
    assert!(table.is_token_valid("other").await.unwrap());
    // Each invalidation increments the running counter.
    assert_eq!(count.load(Ordering::Relaxed), 1);
  }

  #[tokio::test]
  async fn test_remove_expired_only_removes_past() {
    let db_config = DBConfig::default();
    let conn = connect_db(&db_config, "sqlite::memory:").await;
    Migrator::up(&*conn, None).await.unwrap();

    let table = InvalidJwtTable::new(&conn);
    let count = Arc::new(AtomicI32::new(0));

    let expired = Utc::now() - Duration::seconds(3600);
    let valid = Utc::now() + Duration::seconds(3600);
    table
      .invalidate_jwt("expired".into(), expired, count.clone())
      .await
      .unwrap();
    table
      .invalidate_jwt("valid".into(), valid, count.clone())
      .await
      .unwrap();

    table.remove_expired().await.unwrap();

    // remove_expired drops only entries whose expiry is in the past; the
    // still-valid invalidation remains in effect.
    assert!(table.is_token_valid("expired").await.unwrap());
    assert!(!table.is_token_valid("valid").await.unwrap());
  }
}
