use sea_orm::{ActiveValue::Set, IntoActiveModel, prelude::*};

use crate::{db::entities::setup, error::Result};

const SETUP_ID: i32 = 1;

pub struct SetupTable<'db> {
  db: &'db DatabaseConnection,
}

impl<'db> SetupTable<'db> {
  pub fn new(db: &'db DatabaseConnection) -> Self {
    Self { db }
  }

  pub async fn is_setup(&self) -> Result<bool> {
    let res = setup::Entity::find_by_id(SETUP_ID).one(self.db).await?;
    Ok(res.is_some_and(|s| s.completed))
  }

  async fn get_setup(&self) -> Result<setup::Model> {
    let res = setup::Entity::find_by_id(SETUP_ID).one(self.db).await?;

    if let Some(model) = res {
      Ok(model)
    } else {
      Ok(
        setup::ActiveModel {
          id: Set(SETUP_ID),
          admin_group_created: Set(None),
          completed: Set(false),
        }
        .insert(self.db)
        .await?,
      )
    }
  }

  pub async fn mark_completed(&self) -> Result<()> {
    let mut model = self.get_setup().await?.into_active_model();
    model.completed = Set(true);

    model.update(self.db).await?;

    Ok(())
  }

  pub async fn set_admin_group_created(&self, group_id: Uuid) -> Result<()> {
    let mut model = self.get_setup().await?.into_active_model();
    model.admin_group_created = Set(Some(group_id));

    model.update(self.db).await?;

    Ok(())
  }

  pub async fn get_admin_group_id(&self) -> Result<Option<Uuid>> {
    let model = self.get_setup().await?;
    Ok(model.admin_group_created)
  }
}
