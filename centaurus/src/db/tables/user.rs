use eyre::ContextCompat;
use sea_orm::{Set, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{
  db::{
    entities::{group, group_user, user},
    tables::group::{GroupTable, SimpleUserInfo},
  },
  error::Result,
};

pub struct UserTable<'db> {
  db: &'db DatabaseConnection,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema))]
pub struct UserListInfo {
  pub uuid: Uuid,
  pub name: String,
  pub email: String,
  pub groups: Vec<SimpleGroupInfo>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema))]
pub struct DetailUserInfo {
  pub uuid: Uuid,
  pub name: String,
  pub email: String,
  pub groups: Vec<SimpleGroupInfo>,
  pub permissions: Vec<String>,
  pub oidc_user: bool,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema))]
pub struct SimpleGroupInfo {
  pub uuid: Uuid,
  pub name: String,
}

impl<'db> UserTable<'db> {
  pub fn new(db: &'db DatabaseConnection) -> Self {
    Self { db }
  }

  pub async fn create_user(
    &self,
    username: String,
    email: String,
    password: String,
    salt: String,
    oidc_user: bool,
  ) -> Result<Uuid> {
    use sea_orm::IntoActiveModel;

    #[cfg(feature = "avatar")]
    let url = crate::gravatar::get_gravatar_url(&email);
    #[cfg(feature = "avatar")]
    let data = match reqwest::get(&url).await {
      Ok(response) => {
        if response.status().is_success() {
          match response.bytes().await {
            Ok(bytes) => {
              use std::io::Cursor;

              use image::{ImageFormat, imageops::FilterType};

              let img = image::load_from_memory(&bytes)?;
              let img = img.resize_exact(128, 128, FilterType::Lanczos3);

              let mut buf = Cursor::new(Vec::new());
              img.write_to(&mut buf, ImageFormat::WebP)?;
              Some(buf.into_inner())
            }
            Err(_) => None,
          }
        } else {
          None
        }
      }
      Err(_) => None,
    };

    let model = user::Model {
      id: Uuid::new_v4(),
      name: username,
      email,
      password,
      salt,
      oidc_user,
    }
    .into_active_model();

    let ret = model.insert(self.db).await?;

    #[cfg(feature = "avatar")]
    if let Some(data) = data {
      crate::db::entities::user_avatar::Model {
        user_id: ret.id,
        data,
      }
      .into_active_model()
      .insert(self.db)
      .await?;
    }

    Ok(ret.id)
  }

  pub async fn try_get_user_by_email(&self, email: &str) -> Result<Option<user::Model>> {
    Ok(
      user::Entity::find()
        .filter(user::Column::Email.eq(email.to_string()))
        .one(self.db)
        .await?,
    )
  }

  pub async fn get_user_by_email(&self, email: &str) -> Result<user::Model> {
    Ok(
      self
        .try_get_user_by_email(email)
        .await?
        .context(format!("User with email {} not found", email))?,
    )
  }

  pub async fn get_user_by_id(&self, id: Uuid) -> Result<user::Model> {
    Ok(
      user::Entity::find_by_id(id)
        .one(self.db)
        .await?
        .context(format!("User with id {} not found", id))?,
    )
  }

  pub async fn update_user_password(&self, id: Uuid, new_password: String) -> Result<()> {
    let mut user: user::ActiveModel = self.get_user_by_id(id).await?.into();

    user.password = Set(new_password);

    user.update(self.db).await?;

    Ok(())
  }

  pub async fn to_local_user(&self, id: Uuid) -> Result<()> {
    let mut user: user::ActiveModel = self.get_user_by_id(id).await?.into();

    user.oidc_user = Set(false);

    user.update(self.db).await?;

    Ok(())
  }

  pub async fn update_user_name(&self, id: Uuid, new_name: String) -> Result<()> {
    let mut user: user::ActiveModel = self.get_user_by_id(id).await?.into();

    user.name = Set(new_name);

    user.update(self.db).await?;

    Ok(())
  }

  pub async fn list_users_simple(&self) -> Result<Vec<SimpleUserInfo>> {
    let users = user::Entity::find().all(self.db).await?;

    Ok(
      users
        .into_iter()
        .map(|u| SimpleUserInfo {
          id: u.id,
          name: u.name,
        })
        .collect(),
    )
  }

  pub async fn get_user_groups(&self, user_id: Uuid) -> Result<Vec<SimpleGroupInfo>> {
    let groups = group_user::Entity::find()
      .filter(group_user::Column::UserId.eq(user_id))
      .find_also_related(group::Entity)
      .all(self.db)
      .await?
      .into_iter()
      .filter_map(|(_, group)| {
        group.map(|g| SimpleGroupInfo {
          uuid: g.id,
          name: g.name,
        })
      })
      .collect();

    Ok(groups)
  }

  pub async fn user_info(&self, user_id: Uuid) -> Result<Option<DetailUserInfo>> {
    let user = user::Entity::find_by_id(user_id).one(self.db).await?;
    let Some(user) = user else {
      return Ok(None);
    };

    let groups = self.get_user_groups(user_id).await?;
    let permissions = GroupTable::new(self.db)
      .get_user_permissions(user_id)
      .await?;

    Ok(Some(DetailUserInfo {
      uuid: user.id,
      name: user.name,
      email: user.email,
      groups,
      permissions,
      oidc_user: user.oidc_user,
    }))
  }

  pub async fn delete_user(&self, user_id: Uuid) -> Result<()> {
    user::Entity::delete_by_id(user_id).exec(self.db).await?;
    Ok(())
  }

  pub async fn edit_user(
    &self,
    user_id: Uuid,
    new_name: String,
    new_groups: Vec<Uuid>,
  ) -> Result<()> {
    let mut user: user::ActiveModel = self.get_user_by_id(user_id).await?.into();

    user.name = Set(new_name);

    user.update(self.db).await?;

    // Update groups
    self.clear_user_groups(user_id).await?;

    if !new_groups.is_empty() {
      GroupTable::new(self.db)
        .add_user_to_groups(user_id, new_groups)
        .await?;
    }

    Ok(())
  }

  #[cfg(feature = "avatar")]
  pub async fn update_user_avatar(&self, id: Uuid, new_avatar: Vec<u8>) -> Result<()> {
    use crate::db::entities::user_avatar;

    if let Some(avatar_model) = user_avatar::Entity::find_by_id(id).one(self.db).await? {
      let mut avatar_active: user_avatar::ActiveModel = avatar_model.into();
      avatar_active.data = Set(new_avatar);
      avatar_active.update(self.db).await?;
      return Ok(());
    } else {
      use sea_orm::IntoActiveModel;

      crate::db::entities::user_avatar::Model {
        user_id: id,
        data: new_avatar.clone(),
      }
      .into_active_model()
      .insert(self.db)
      .await?;
    }

    Ok(())
  }

  #[cfg(feature = "avatar")]
  pub async fn reset_avatar(&self, user_id: Uuid) -> Result<()> {
    use crate::db::entities::user_avatar;

    user_avatar::Entity::delete_by_id(user_id)
      .exec(self.db)
      .await?;
    Ok(())
  }

  #[cfg(feature = "avatar")]
  pub async fn get_user_avatar(&self, user_id: Uuid) -> Result<Option<Vec<u8>>> {
    use crate::db::entities::user_avatar;

    let avatar = user_avatar::Entity::find_by_id(user_id)
      .one(self.db)
      .await?;

    Ok(avatar.map(|a| a.data))
  }

  pub async fn list_users(&self) -> Result<Vec<UserListInfo>> {
    let users = user::Entity::find().all(self.db).await?;
    let group_user = users
      .load_many_to_many(group::Entity, group_user::Entity, self.db)
      .await?;

    let result = users
      .into_iter()
      .zip(group_user)
      .map(|(user, groups)| UserListInfo {
        uuid: user.id,
        name: user.name,
        email: user.email,
        groups: groups
          .into_iter()
          .map(|group| SimpleGroupInfo {
            uuid: group.id,
            name: group.name,
          })
          .collect(),
      })
      .collect();

    Ok(result)
  }

  pub async fn change_email(&self, uuid: Uuid, new_email: String) -> Result<()> {
    let mut user: user::ActiveModel = self.get_user_by_id(uuid).await?.into();

    user.email = Set(new_email.to_lowercase());

    user.update(self.db).await?;

    Ok(())
  }

  pub async fn clear_user_groups(&self, user_id: Uuid) -> Result<()> {
    group_user::Entity::delete_many()
      .filter(group_user::Column::UserId.eq(user_id))
      .exec(self.db)
      .await?;

    Ok(())
  }

  pub async fn count_users(&self) -> Result<u64> {
    let count = user::Entity::find().count(self.db).await?;
    Ok(count)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::config::DBConfig;
  use crate::db::init::{Connection, connect_db};
  use crate::db::migrations::Migrator;
  use crate::db::tables::group::GroupTable;
  use sea_orm_migration::MigratorTrait;

  async fn setup() -> Connection {
    let db_config = DBConfig::default();
    let conn = connect_db(&db_config, "sqlite::memory:").await;
    Migrator::up(&*conn, None).await.unwrap();
    conn
  }

  async fn make_user(table: &UserTable<'_>, name: &str) -> Uuid {
    table
      .create_user(
        name.into(),
        format!("{name}@example.com"),
        "pass".into(),
        "salt".into(),
        false,
      )
      .await
      .unwrap()
  }

  #[tokio::test]
  async fn test_user_table() {
    let conn = setup().await;

    let table = UserTable::new(&conn);
    let id = make_user(&table, "test").await;

    let user = table.get_user_by_id(id).await.unwrap();
    assert_eq!(user.name, "test");
    assert_eq!(user.email, "test@example.com");

    let count = table.count_users().await.unwrap();
    assert_eq!(count, 1);
  }

  #[tokio::test]
  async fn test_lookup_by_email() {
    let conn = setup().await;
    let table = UserTable::new(&conn);
    let id = make_user(&table, "lookup").await;

    let user = table.get_user_by_email("lookup@example.com").await.unwrap();
    assert_eq!(user.id, id);

    // The Option variant returns None instead of erroring for unknown emails.
    assert!(
      table
        .try_get_user_by_email("missing@example.com")
        .await
        .unwrap()
        .is_none()
    );
    assert!(table.get_user_by_email("missing@example.com").await.is_err());
  }

  #[tokio::test]
  async fn test_missing_user_errors() {
    let conn = setup().await;
    let table = UserTable::new(&conn);
    assert!(table.get_user_by_id(Uuid::new_v4()).await.is_err());
    // user_info resolves to None rather than erroring for an unknown id.
    assert!(table.user_info(Uuid::new_v4()).await.unwrap().is_none());
  }

  #[tokio::test]
  async fn test_update_fields() {
    let conn = setup().await;
    let table = UserTable::new(&conn);
    let id = table
      .create_user(
        "orig".into(),
        "orig@example.com".into(),
        "pw".into(),
        "salt".into(),
        true,
      )
      .await
      .unwrap();

    table.update_user_name(id, "renamed".into()).await.unwrap();
    table
      .update_user_password(id, "newpw".into())
      .await
      .unwrap();
    assert!(table.to_local_user(id).await.is_ok());

    let user = table.get_user_by_id(id).await.unwrap();
    assert_eq!(user.name, "renamed");
    assert_eq!(user.password, "newpw");
    // to_local_user must clear the oidc flag.
    assert!(!user.oidc_user);
  }

  #[tokio::test]
  async fn test_change_email_lowercases() {
    let conn = setup().await;
    let table = UserTable::new(&conn);
    let id = make_user(&table, "case").await;

    // change_email normalizes to lowercase regardless of input casing.
    table
      .change_email(id, "MixedCase@Example.COM".into())
      .await
      .unwrap();
    let user = table.get_user_by_id(id).await.unwrap();
    assert_eq!(user.email, "mixedcase@example.com");
  }

  #[tokio::test]
  async fn test_groups_and_info() {
    let conn = setup().await;
    let table = UserTable::new(&conn);
    let group_table = GroupTable::new(&conn);
    let id = make_user(&table, "member").await;
    let group = group_table.create_group("grp".into()).await.unwrap();
    group_table.add_users_to_group(group, vec![id]).await.unwrap();
    group_table
      .add_permissions_to_group(group, vec!["perm".into()])
      .await
      .unwrap();

    let groups = table.get_user_groups(id).await.unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].uuid, group);

    let info = table.user_info(id).await.unwrap().unwrap();
    assert_eq!(info.groups.len(), 1);
    assert_eq!(info.permissions, vec!["perm".to_string()]);

    // clear_user_groups removes membership without deleting the user.
    table.clear_user_groups(id).await.unwrap();
    assert!(table.get_user_groups(id).await.unwrap().is_empty());
    assert!(table.get_user_by_id(id).await.is_ok());
  }

  #[tokio::test]
  async fn test_edit_user_replaces_groups() {
    let conn = setup().await;
    let table = UserTable::new(&conn);
    let group_table = GroupTable::new(&conn);
    let id = make_user(&table, "edit").await;
    let g1 = group_table.create_group("g1".into()).await.unwrap();
    let g2 = group_table.create_group("g2".into()).await.unwrap();

    table.edit_user(id, "newname".into(), vec![g1]).await.unwrap();
    assert_eq!(table.get_user_by_id(id).await.unwrap().name, "newname");
    assert_eq!(table.get_user_groups(id).await.unwrap().len(), 1);

    // Re-editing replaces the previous group set entirely.
    table.edit_user(id, "newname".into(), vec![g2]).await.unwrap();
    let groups = table.get_user_groups(id).await.unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].uuid, g2);

    // Editing with an empty group list clears membership.
    table.edit_user(id, "newname".into(), vec![]).await.unwrap();
    assert!(table.get_user_groups(id).await.unwrap().is_empty());
  }

  #[tokio::test]
  async fn test_list_users() {
    let conn = setup().await;
    let table = UserTable::new(&conn);
    make_user(&table, "u1").await;
    make_user(&table, "u2").await;

    assert_eq!(table.count_users().await.unwrap(), 2);
    assert_eq!(table.list_users().await.unwrap().len(), 2);
    assert_eq!(table.list_users_simple().await.unwrap().len(), 2);
  }

  #[tokio::test]
  async fn test_delete_user() {
    let conn = setup().await;
    let table = UserTable::new(&conn);
    let id = make_user(&table, "del").await;

    table.delete_user(id).await.unwrap();
    assert_eq!(table.count_users().await.unwrap(), 0);
    // Deleting an already-absent user is idempotent (no error).
    table.delete_user(id).await.unwrap();
  }

  #[cfg(feature = "avatar")]
  #[tokio::test]
  async fn test_avatar_roundtrip() {
    let conn = setup().await;
    let table = UserTable::new(&conn);
    let id = make_user(&table, "avatar").await;

    table.update_user_avatar(id, vec![1, 2, 3]).await.unwrap();
    assert_eq!(
      table.get_user_avatar(id).await.unwrap(),
      Some(vec![1, 2, 3])
    );

    // Updating again overwrites the stored avatar.
    table.update_user_avatar(id, vec![4, 5]).await.unwrap();
    assert_eq!(table.get_user_avatar(id).await.unwrap(), Some(vec![4, 5]));

    table.reset_avatar(id).await.unwrap();
    assert_eq!(table.get_user_avatar(id).await.unwrap(), None);
  }
}
