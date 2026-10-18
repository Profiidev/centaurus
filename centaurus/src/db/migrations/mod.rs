use sea_orm_migration::prelude::*;

pub mod m0_key;
pub mod m1_invalid_jwt;
pub mod m2_settings;
pub mod m3_user;
pub mod m4_groups;
pub mod m5_setup;
pub mod m6_user_oidc_subject;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
  fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![
      Box::new(m0_key::Migration),
      Box::new(m1_invalid_jwt::Migration),
      Box::new(m2_settings::Migration),
      Box::new(m3_user::Migration),
      Box::new(m4_groups::Migration),
      Box::new(m5_setup::Migration),
      Box::new(m6_user_oidc_subject::Migration),
    ]
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::{config::DBConfig, init::connect_db};

  #[tokio::test]
  async fn migrations_full_down_up_sqlite() {
    let conn = connect_db(&DBConfig::default(), "sqlite::memory:").await;
    Migrator::up(&*conn, None).await.unwrap();
    // Rolling back every migration must produce valid SQLite SQL.
    Migrator::down(&*conn, None).await.unwrap();
    assert!(
      Migrator::get_applied_migrations(&*conn)
        .await
        .unwrap()
        .is_empty()
    );
    Migrator::up(&*conn, None).await.unwrap();
  }
}
