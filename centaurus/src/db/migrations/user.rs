use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

const EMAIL_INDEX_NAME: &str = "user.user_email";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .create_table(
        Table::create()
          .table(User::Table)
          .if_not_exists()
          .col(pk_uuid(User::Id))
          .col(string(User::Name))
          .col(string(User::Email))
          .col(string(User::Password))
          .col(string(User::Salt))
          .col(string_null(User::Avatar))
          .to_owned(),
      )
      .await?;

    manager
      .create_index(
        Index::create()
          .name(EMAIL_INDEX_NAME)
          .table(User::Table)
          .col(User::Email)
          .unique()
          .to_owned(),
      )
      .await
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .drop_index(Index::drop().name(EMAIL_INDEX_NAME).to_owned())
      .await?;

    manager
      .drop_table(Table::drop().table(User::Table).to_owned())
      .await
  }
}

#[derive(DeriveIden)]
pub enum User {
  Table,
  Id,
  Name,
  Email,
  Password,
  Salt,
  Avatar,
}
