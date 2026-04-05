use aide::axum::ApiRouter;
use aide::axum::routing::{get_with, post_with};
use axum::Json;
use http::StatusCode;

use crate::backend::auth::jwt_auth::JwtAuth;
use crate::backend::auth::oidc::OidcState;
use crate::backend::auth::permission::{SettingsEdit, SettingsView};
use crate::backend::auth::settings::UserSettings;
use crate::backend::websocket::state::{UpdateMessage, Updater};
use crate::db::init::Connection;
use crate::db::settings::Settings;
use crate::db::tables::ConnectionExt;
use crate::error::{ErrorReportStatusExt, Result};
use crate::mail::{MailSettings, Mailer};

pub fn router<T: UpdateMessage>() -> ApiRouter {
  ApiRouter::new()
    .api_route(
      "/user",
      get_with(get_settings::<UserSettings>, |op| op.id("getUserSettings")),
    )
    .api_route(
      "/user",
      post_with(save_user_settings::<T>, |op| op.id("saveUserSettings")),
    )
    .api_route(
      "/mail",
      get_with(get_settings::<MailSettings>, |op| op.id("getMailSettings")),
    )
    .api_route(
      "/mail",
      post_with(save_mail_settings::<T>, |op| op.id("saveMailSettings")),
    )
}

async fn get_settings<S: Settings>(
  _auth: JwtAuth<SettingsView>,
  db: Connection,
) -> Result<Json<S>> {
  Ok(Json(db.settings().get_settings::<S>().await?))
}

#[allow(unused)]
async fn save_settings<S: Settings, T: UpdateMessage>(
  _auth: JwtAuth<SettingsEdit>,
  db: Connection,
  updater: Updater<T>,
  Json(settings): Json<S>,
) -> Result<()> {
  db.settings().save_settings(&settings).await?;
  updater.broadcast(T::settings()).await;
  Ok(())
}

async fn save_user_settings<T: UpdateMessage>(
  _auth: JwtAuth<SettingsEdit>,
  db: Connection,
  state: OidcState,
  updater: Updater<T>,
  Json(settings): Json<UserSettings>,
) -> Result<()> {
  if let Some(oidc_settings) = &settings.oidc {
    state.try_init(oidc_settings).await.status_context(
      StatusCode::NOT_ACCEPTABLE,
      "Failed to initialize OIDC state",
    )?;
  } else {
    state.deactivate().await;
  }

  db.settings().save_settings(&settings).await?;
  updater.broadcast(T::settings()).await;

  Ok(())
}

async fn save_mail_settings<T: UpdateMessage>(
  _auth: JwtAuth<SettingsEdit>,
  db: Connection,
  state: Mailer,
  updater: Updater<T>,
  Json(settings): Json<MailSettings>,
) -> Result<()> {
  if let Some(smtp_settings) = &settings.smtp {
    state.try_init(smtp_settings).await?;
  } else {
    state.deactivate().await;
  }

  db.settings().save_settings(&settings).await?;
  updater.broadcast(T::settings()).await;

  Ok(())
}
