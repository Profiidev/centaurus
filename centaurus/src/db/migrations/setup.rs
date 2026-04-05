use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .create_table(
        Table::create()
          .table(Setup::Table)
          .if_not_exists()
          .col(pk_auto(Setup::Id))
          .col(uuid_null(Setup::AdminGroupCreated))
          .col(boolean(Setup::Completed))
          .to_owned(),
      )
      .await
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .drop_table(Table::drop().table(Setup::Table).to_owned())
      .await
  }
}

#[derive(DeriveIden)]
pub enum Setup {
  Table,
  Id,
  AdminGroupCreated,
  Completed,
}
