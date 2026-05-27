use aide::axum::routing::{ApiMethodRouter, get_with, post_with};
use axum::Json;
use http::StatusCode;
use schemars::JsonSchema;
use serde::Serialize;

use crate::backend::BackendRouter;
use crate::backend::auth::jwt_auth::JwtAuth;
use crate::backend::auth::oidc::OidcState;
use crate::backend::auth::permission::{SettingsEdit, SettingsView};
use crate::backend::auth::settings::UserSettings;
use crate::backend::endpoints::websocket::state::{UpdateMessage, Updater};
use crate::db::init::Connection;
use crate::db::tables::ConnectionExt;
use crate::error::{ErrorReportStatusExt, Result};
#[cfg(feature = "mail")]
use crate::mail::{MailSettings, Mailer};
use crate::overwrite_with_env_config;

pub fn router<T: UpdateMessage>() -> BackendRouter {
  let router = BackendRouter::new()
    .api_route("/user", get_user_settings_route())
    .api_route("/user", save_user_settings_route::<T>());

  #[cfg(feature = "mail")]
  {
    router
      .api_route("/mail", get_mail_settings_route())
      .api_route("/mail", save_mail_settings_route::<T>())
  }

  #[cfg(not(feature = "mail"))]
  {
    router
  }
}

pub fn get_user_settings_route() -> ApiMethodRouter<()> {
  get_with(get_user_settings, |op| op.id("getUserSettings"))
}

pub fn save_user_settings_route<T: UpdateMessage>() -> ApiMethodRouter<()> {
  post_with(save_user_settings::<T>, |op| op.id("saveUserSettings"))
}

#[cfg(feature = "mail")]
pub fn get_mail_settings_route() -> ApiMethodRouter<()> {
  get_with(get_mail_settings, |op| op.id("getMailSettings"))
}

#[cfg(feature = "mail")]
pub fn save_mail_settings_route<T: UpdateMessage>() -> ApiMethodRouter<()> {
  post_with(save_mail_settings::<T>, |op| op.id("saveMailSettings"))
}

#[derive(Serialize, JsonSchema)]
struct UserSettingsResponse {
  settings: UserSettings,
  from_env: Vec<String>,
}

#[macro_export]
macro_rules! each_field_from_env {
  ($type:tt, $config:ident, $env_config:ident, $($field:ident),*,, $($bool_field:ident),*) => {
    {
      if let Some($env_config) = $env_config {
        let mut from_env = Vec::new();
        $(
          if let Some($field) = &$env_config.$field {
            from_env.push(stringify!($field).to_string());
            $config.$field = Some($field.clone());
          }
        )*

        $(
          if $env_config.$bool_field {
            from_env.push(stringify!($bool_field).to_string());
            $config.$bool_field = true;
          }
        )*

        $type {
          settings: $config,
          from_env,
        }
      } else {
        $type {
          settings: $config,
          from_env: Vec::new(),
        }
      }
    }
  };
}

async fn get_user_settings(
  _auth: JwtAuth<SettingsView>,
  db: Connection,
  config: Option<UserSettings>,
) -> Result<Json<UserSettingsResponse>> {
  let mut settings = db.settings().get_settings::<UserSettings>().await?;

  let res = each_field_from_env!(
    UserSettingsResponse,
    settings,
    config,
    oidc_issuer,
    oidc_client_id,
    oidc_client_secret,
    oidc_scopes,,
    oidc_enabled,
    oidc_group_sync
  );

  Ok(Json(res))
}

#[cfg(feature = "mail")]
#[derive(Serialize, JsonSchema)]
struct MailSettingsResponse {
  settings: MailSettings,
  from_env: Vec<String>,
}

#[cfg(feature = "mail")]
async fn get_mail_settings(
  _auth: JwtAuth<SettingsView>,
  db: Connection,
  config: Option<MailSettings>,
) -> Result<Json<MailSettingsResponse>> {
  let mut settings = db.settings().get_settings::<MailSettings>().await?;

  let res = each_field_from_env!(
    MailSettingsResponse,
    settings,
    config,
    smtp_server,
    smtp_port,
    smtp_username,
    smtp_password,
    smtp_from_address,
    smtp_from_name,
    smtp_use_tls,,
    smtp_enabled
  );

  Ok(Json(res))
}

async fn save_user_settings<T: UpdateMessage>(
  _auth: JwtAuth<SettingsEdit>,
  db: Connection,
  state: OidcState,
  updater: Updater<T>,
  config: Option<UserSettings>,
  Json(mut settings): Json<UserSettings>,
) -> Result<()> {
  overwrite_with_env_config!(
    settings,
    config,
    oidc_issuer,
    oidc_client_id,
    oidc_client_secret,
    oidc_scopes,,
    oidc_enabled,
    oidc_group_sync,
    sso_create_user,
    sso_instant_redirect
  );

  if let Some(oidc_settings) = &settings.oidc_settings() {
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

#[cfg(feature = "mail")]
async fn save_mail_settings<T: UpdateMessage>(
  _auth: JwtAuth<SettingsEdit>,
  db: Connection,
  state: Mailer,
  updater: Updater<T>,
  config: Option<MailSettings>,
  Json(mut settings): Json<MailSettings>,
) -> Result<()> {
  overwrite_with_env_config!(
    settings,
    config,
    smtp_server,
    smtp_port,
    smtp_username,
    smtp_password,
    smtp_from_address,
    smtp_from_name,
    smtp_use_tls,,
    smtp_enabled
  );

  if let Some(smtp_settings) = &settings.smtp() {
    state.try_init(smtp_settings).await?;
  } else {
    state.deactivate().await;
  }

  db.settings().save_settings(&settings).await?;
  updater.broadcast(T::settings()).await;

  Ok(())
}
