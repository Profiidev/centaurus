use sea_orm_migration::{prelude::*, schema::*};

use crate::db::migrations::m3_user::User;

#[derive(DeriveMigrationName)]
pub struct Migration;

const OIDC_SUBJECT_INDEX_NAME: &str = "user.user_oidc_subject";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .alter_table(
        Table::alter()
          .table(User::Table)
          .add_column(string_null(User::OidcSubject))
          .to_owned(),
      )
      .await?;

    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name(OIDC_SUBJECT_INDEX_NAME)
          .table(User::Table)
          .col(User::OidcSubject)
          .unique()
          .to_owned(),
      )
      .await
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .drop_index(Index::drop().name(OIDC_SUBJECT_INDEX_NAME).to_owned())
      .await?;

    manager
      .alter_table(
        Table::alter()
          .table(User::Table)
          .drop_column(User::OidcSubject)
          .to_owned(),
      )
      .await
  }
}
