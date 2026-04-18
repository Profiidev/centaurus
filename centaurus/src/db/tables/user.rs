use sea_orm::{Set, prelude::*};
use serde::{Deserialize, Serialize};

use crate::db::{
  entities::{group, group_user, user},
  tables::group::{GroupTable, SimpleUserInfo},
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
  pub avatar: Option<String>,
  pub groups: Vec<SimpleGroupInfo>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema))]
pub struct DetailUserInfo {
  pub uuid: Uuid,
  pub name: String,
  pub email: String,
  pub avatar: Option<String>,
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

  #[cfg(all(feature = "image", feature = "gravatar"))]
  pub async fn create_user(
    &self,
    username: String,
    email: String,
    password: String,
    salt: String,
  ) -> crate::error::Result<Uuid> {
    use sea_orm::IntoActiveModel;

    let url = crate::gravatar::get_gravatar_url(&email);
    let data = match reqwest::get(&url).await {
      Ok(response) => {
        if response.status().is_success() {
          match response.bytes().await {
            Ok(bytes) => {
              use std::io::Cursor;

              use base64::{Engine, prelude::BASE64_STANDARD};
              use image::{ImageFormat, imageops::FilterType};

              let img = image::load_from_memory(&bytes)?;
              let img = img.resize_exact(128, 128, FilterType::Lanczos3);

              let mut buf = Cursor::new(Vec::new());
              img.write_to(&mut buf, ImageFormat::WebP)?;
              let avatar = BASE64_STANDARD.encode(buf.into_inner());
              Some(avatar)
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
      avatar: data,
    }
    .into_active_model();

    let ret = model.insert(self.db).await?;

    Ok(ret.id)
  }

  pub async fn try_get_user_by_email(&self, email: &str) -> Result<Option<user::Model>, DbErr> {
    user::Entity::find()
      .filter(user::Column::Email.eq(email.to_string()))
      .one(self.db)
      .await
  }

  pub async fn get_user_by_email(&self, email: &str) -> Result<user::Model, DbErr> {
    self
      .try_get_user_by_email(email)
      .await?
      .ok_or(DbErr::RecordNotFound(format!(
        "User with email {} not found",
        email
      )))
  }

  pub async fn get_user_by_id(&self, id: Uuid) -> Result<user::Model, DbErr> {
    user::Entity::find_by_id(id)
      .one(self.db)
      .await?
      .ok_or(DbErr::RecordNotFound(format!(
        "User with id {} not found",
        id
      )))
  }

  pub async fn update_user_password(&self, id: Uuid, new_password: String) -> Result<(), DbErr> {
    let mut user: user::ActiveModel = self.get_user_by_id(id).await?.into();

    user.password = Set(new_password);

    user.update(self.db).await?;

    Ok(())
  }

  pub async fn update_user_name(&self, id: Uuid, new_name: String) -> Result<(), DbErr> {
    let mut user: user::ActiveModel = self.get_user_by_id(id).await?.into();

    user.name = Set(new_name);

    user.update(self.db).await?;

    Ok(())
  }

  pub async fn list_users_simple(&self) -> Result<Vec<SimpleUserInfo>, DbErr> {
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

  pub async fn get_user_groups(&self, user_id: Uuid) -> Result<Vec<SimpleGroupInfo>, DbErr> {
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

  pub async fn user_info(&self, user_id: Uuid) -> Result<Option<DetailUserInfo>, DbErr> {
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
      avatar: user.avatar,
      groups,
      permissions,
    }))
  }

  pub async fn delete_user(&self, user_id: Uuid) -> Result<(), DbErr> {
    user::Entity::delete_by_id(user_id).exec(self.db).await?;
    Ok(())
  }

  pub async fn edit_user(
    &self,
    user_id: Uuid,
    new_name: String,
    new_groups: Vec<Uuid>,
  ) -> Result<(), DbErr> {
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

  pub async fn update_user_avatar(&self, id: Uuid, new_avatar: String) -> Result<(), DbErr> {
    let mut user: user::ActiveModel = self.get_user_by_id(id).await?.into();

    user.avatar = Set(Some(new_avatar));

    user.update(self.db).await?;

    Ok(())
  }

  pub async fn reset_avatar(&self, user_id: Uuid) -> Result<(), DbErr> {
    let mut user: user::ActiveModel = self.get_user_by_id(user_id).await?.into();

    user.avatar = Set(None);

    user.update(self.db).await?;

    Ok(())
  }

  pub async fn list_users(&self) -> Result<Vec<UserListInfo>, DbErr> {
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
        avatar: user.avatar,
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
}
