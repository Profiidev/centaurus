use sea_orm_migration::prelude::*;

pub mod m0_key;
pub mod m1_invalid_jwt;
pub mod m2_settings;
pub mod m3_user;
pub mod m4_groups;
pub mod m5_setup;

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
    ]
  }
}
