use sea_orm::{IntoActiveModel, Set, prelude::*};

use crate::{
  db::{entities::settings, settings::Settings},
  error::ErrorReport,
};

pub struct SettingsTable<'db> {
  db: &'db DatabaseConnection,
}

impl<'db> SettingsTable<'db> {
  pub fn new(db: &'db DatabaseConnection) -> Self {
    Self { db }
  }

  pub async fn get_settings<S: Settings>(&self) -> Result<S, ErrorReport> {
    let res = settings::Entity::find_by_id(S::id()).one(self.db).await?;
    let Some(model) = res else {
      return Ok(S::default());
    };

    Ok(serde_json::from_str(&model.content)?)
  }

  pub async fn save_settings<S: Settings>(&self, settings: &S) -> Result<(), ErrorReport> {
    let content = serde_json::to_string(settings)?;

    match settings::Entity::find_by_id(S::id()).one(self.db).await? {
      Some(m) => {
        let mut am = m.into_active_model();
        am.content = Set(content);
        am.update(self.db).await?;
      }
      None => {
        let model = settings::Model {
          id: S::id(),
          content,
        };

        model.into_active_model().insert(self.db).await?;
      }
    };

    Ok(())
  }
}
