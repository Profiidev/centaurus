use sea_orm_migration::{prelude::*, schema::*};

use crate::db::migrations::user::User;

#[derive(DeriveMigrationName)]
pub struct Migration;

const GROUP_USER_GROUP_ID_INDEX_NAME: &str = "group_user.group_id";
const GROUP_USER_USER_ID_INDEX_NAME: &str = "group_user.user_id";
const GROUP_PERMISSION_GROUP_ID_INDEX_NAME: &str = "group_permission.group_id";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .create_table(
        Table::create()
          .table(Group::Table)
          .if_not_exists()
          .col(pk_uuid(Group::Id))
          .col(string(Group::Name))
          .to_owned(),
      )
      .await?;

    manager
      .create_table(
        Table::create()
          .table(GroupUser::Table)
          .if_not_exists()
          .primary_key(
            Index::create()
              .table(GroupUser::Table)
              .col(GroupUser::GroupId)
              .col(GroupUser::UserId),
          )
          .col(uuid(GroupUser::GroupId))
          .col(uuid(GroupUser::UserId))
          .foreign_key(
            ForeignKey::create()
              .from(GroupUser::Table, GroupUser::GroupId)
              .to(Group::Table, Group::Id)
              .on_delete(ForeignKeyAction::Cascade)
              .on_update(ForeignKeyAction::Cascade),
          )
          .foreign_key(
            ForeignKey::create()
              .from(GroupUser::Table, GroupUser::UserId)
              .to(User::Table, User::Id)
              .on_delete(ForeignKeyAction::Cascade)
              .on_update(ForeignKeyAction::Cascade),
          )
          .to_owned(),
      )
      .await?;

    manager
      .create_table(
        Table::create()
          .table(GroupPermission::Table)
          .if_not_exists()
          .primary_key(
            Index::create()
              .table(GroupPermission::Table)
              .col(GroupPermission::GroupId)
              .col(GroupPermission::Permission),
          )
          .col(uuid(GroupPermission::GroupId))
          .col(string(GroupPermission::Permission))
          .foreign_key(
            ForeignKey::create()
              .from(GroupPermission::Table, GroupPermission::GroupId)
              .to(Group::Table, Group::Id)
              .on_delete(ForeignKeyAction::Cascade)
              .on_update(ForeignKeyAction::Cascade),
          )
          .to_owned(),
      )
      .await?;

    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name(GROUP_USER_GROUP_ID_INDEX_NAME)
          .table(GroupUser::Table)
          .col(GroupUser::GroupId)
          .to_owned(),
      )
      .await?;

    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name(GROUP_USER_USER_ID_INDEX_NAME)
          .table(GroupUser::Table)
          .col(GroupUser::UserId)
          .to_owned(),
      )
      .await?;

    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name(GROUP_PERMISSION_GROUP_ID_INDEX_NAME)
          .table(GroupPermission::Table)
          .col(GroupPermission::GroupId)
          .to_owned(),
      )
      .await
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .drop_index(
        Index::drop()
          .name(GROUP_USER_GROUP_ID_INDEX_NAME)
          .to_owned(),
      )
      .await?;

    manager
      .drop_index(Index::drop().name(GROUP_USER_USER_ID_INDEX_NAME).to_owned())
      .await?;

    manager
      .drop_index(
        Index::drop()
          .name(GROUP_PERMISSION_GROUP_ID_INDEX_NAME)
          .to_owned(),
      )
      .await?;

    manager
      .drop_table(
        Table::drop()
          .table(GroupPermission::Table)
          .table(GroupUser::Table)
          .table(Group::Table)
          .to_owned(),
      )
      .await
  }
}

#[derive(DeriveIden)]
pub enum Group {
  Table,
  Id,
  Name,
}

#[derive(DeriveIden)]
pub enum GroupUser {
  Table,
  GroupId,
  UserId,
}

#[derive(DeriveIden)]
pub enum GroupPermission {
  Table,
  GroupId,
  Permission,
}
