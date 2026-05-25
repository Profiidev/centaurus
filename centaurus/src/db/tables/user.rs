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
    group_user::Entity::delete_many()
      .filter(group_user::Column::UserId.eq(user_id))
      .exec(self.db)
      .await?;

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
}
