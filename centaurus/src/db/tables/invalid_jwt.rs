use std::sync::{
  Arc,
  atomic::{AtomicI32, Ordering},
};

use chrono::{DateTime, Utc};
use sea_orm::{ActiveValue::Set, prelude::*};
use tracing::instrument;

use crate::db::entities::invalid_jwt;

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
  ) -> Result<(), DbErr> {
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
  pub async fn is_token_valid(&self, token: &str) -> Result<bool, DbErr> {
    let res = invalid_jwt::Entity::find()
      .filter(invalid_jwt::Column::Token.eq(token))
      .one(self.db)
      .await?;

    Ok(res.is_none())
  }

  #[instrument(skip(self))]
  pub async fn remove_expired(&self) -> Result<(), DbErr> {
    invalid_jwt::Entity::delete_many()
      .filter(invalid_jwt::Column::Exp.lt(Utc::now().naive_utc()))
      .exec(self.db)
      .await?;

    Ok(())
  }
}
