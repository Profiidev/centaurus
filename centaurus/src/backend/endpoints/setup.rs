use aide::axum::ApiRouter;
use aide::axum::routing::{ApiMethodRouter, get_with, post_with};
use argon2::password_hash::SaltString;
use axum::Json;
use axum_extra::extract::CookieJar;
use http::StatusCode;
use rsa::rand_core::OsRng;
use schemars::JsonSchema;
use sea_orm::ConnectionTrait;
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

use crate::backend::auth::jwt_state::JwtState;
use crate::backend::auth::oidc::OidcState;
use crate::backend::auth::pw_state::PasswordState;
use crate::backend::auth::settings::UserSettings;
use crate::backend::endpoints::settings::UserSettingsResponse;
use crate::db::init::Connection;
use crate::db::tables::ConnectionExt;
use crate::error::{ErrorReportStatusExt, Result};
use crate::{bail, each_field_from_env, overwrite_with_env_config};

pub fn router() -> ApiRouter {
  ApiRouter::new()
    .api_route("/", complete_setup_route())
    .api_route("/", is_setup_route())
    .api_route("/oidc", get_oidc_settings_route())
    .api_route("/oidc", init_oidc_route())
}

pub fn complete_setup_route() -> ApiMethodRouter<()> {
  post_with(complete_setup, |op| op.id("completeSetup"))
}

pub fn is_setup_route() -> ApiMethodRouter<()> {
  get_with(is_setup, |op| op.id("isSetup"))
}

pub fn get_oidc_settings_route() -> ApiMethodRouter<()> {
  get_with(get_oidc_settings, |op| op.id("getOidcSettings"))
}

pub fn init_oidc_route() -> ApiMethodRouter<()> {
  post_with(init_oidc, |op| op.id("initOidc"))
}

pub async fn create_admin_group(db: &Connection, all_perms: Vec<&'static str>) -> Result<()> {
  match db.setup().get_admin_group_id().await? {
    Some(id) => {
      info!("Admin group already created with ID {}", id);
      info!("Adding missing permissions to admin group");

      let existing_perms = db.group().get_group_permissions(id).await?;
      let missing_perms: Vec<String> = all_perms
        .into_iter()
        .filter(|p| !existing_perms.contains(&p.to_string()))
        .map(|p| p.to_string())
        .collect();

      if !missing_perms.is_empty() {
        db.group()
          .add_permissions_to_group(id, missing_perms)
          .await?;
        info!("Added missing permissions to admin group");
      } else {
        info!("No missing permissions for admin group");
      }
    }
    None => {
      info!("Admin group not found, creating it with all permissions");

      let all_perms: Vec<String> = all_perms.into_iter().map(|p| p.to_string()).collect();
      let admin_group_id = db.group().create_group("Admin".to_string()).await?;
      db.group()
        .add_permissions_to_group(admin_group_id, all_perms)
        .await?;

      db.setup().set_admin_group_created(admin_group_id).await?;
      info!("Created admin group with ID {}", admin_group_id);
    }
  }

  Ok(())
}

#[derive(Deserialize, JsonSchema)]
struct SetupPayload {
  admin_username: String,
  admin_password: String,
  admin_email: String,
}

#[derive(Serialize, JsonSchema)]
struct SetupResponse {
  user: Uuid,
}

async fn complete_setup(
  db: Connection,
  jwt: JwtState,
  state: PasswordState,
  mut cookies: CookieJar,
  Json(payload): Json<SetupPayload>,
) -> Result<(CookieJar, Json<SetupResponse>)> {
  if db.setup().is_setup().await? {
    bail!(CONFLICT, "Setup has already been completed");
  }

  if payload.admin_username.trim().is_empty() {
    bail!(BAD_REQUEST, "Admin username cannot be empty");
  }

  if payload.admin_email.trim().is_empty() {
    bail!(BAD_REQUEST, "Admin email cannot be empty");
  }

  let Some(admin_group_id) = db.setup().get_admin_group_id().await? else {
    bail!(
      INTERNAL_SERVER_ERROR,
      "Admin group has not been created yet"
    );
  };

  let salt = SaltString::generate(OsRng {}).to_string();
  let hash = state.pw_hash(&salt, &payload.admin_password)?;

  let admin = db
    .user()
    .create_user(
      payload.admin_username,
      payload.admin_email,
      hash,
      salt,
      false,
    )
    .await?;
  db.group()
    .add_user_to_groups(admin, vec![admin_group_id])
    .await?;

  db.setup().mark_completed().await?;
  info!("Setup completed, created admin user with ID {}", admin);

  let cookie = jwt.create_token(admin)?;
  cookies = cookies.add(cookie);
  info!("Created post setup login token for admin user");

  Ok((cookies, Json(SetupResponse { user: admin })))
}

#[derive(Serialize, JsonSchema)]
struct IsSetupResponse {
  is_setup: bool,
  db_backend: String,
  #[cfg(feature = "storage")]
  storage_backend: String,
}

async fn is_setup(
  db: Connection,
  #[cfg(feature = "storage")] storage: crate::storage::FileStorage,
) -> Result<Json<IsSetupResponse>> {
  let db_backend = match db.0.get_database_backend() {
    sea_orm::DatabaseBackend::Postgres => "PostgreSQL",
    sea_orm::DatabaseBackend::MySql => "MySQL",
    sea_orm::DatabaseBackend::Sqlite => "SQLite",
  }
  .to_string();

  Ok(Json(IsSetupResponse {
    is_setup: db.setup().is_setup().await?,
    db_backend,
    #[cfg(feature = "storage")]
    storage_backend: storage.name().to_string(),
  }))
}

async fn get_oidc_settings(
  db: Connection,
  config: Option<UserSettings>,
) -> Result<Json<UserSettingsResponse>> {
  if db.setup().is_setup().await? {
    bail!(FORBIDDEN, "Setup has already been completed");
  }

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
    oidc_image_sync,
    oidc_group_sync,
    sso_instant_redirect,
    sso_create_user
  );

  Ok(Json(res))
}

async fn init_oidc(
  db: Connection,
  state: OidcState,
  config: Option<UserSettings>,
  Json(mut settings): Json<UserSettings>,
) -> Result<()> {
  if db.setup().is_setup().await? {
    bail!(FORBIDDEN, "Setup has already been completed");
  }

  let mut settings_to_db = settings.clone();

  overwrite_with_env_config!(
    settings,
    config,
    oidc_issuer,
    oidc_client_id,
    oidc_client_secret,
    oidc_scopes,,
    oidc_enabled,
    oidc_image_sync,
    oidc_group_sync,
    sso_create_user,
    sso_instant_redirect
  );

  // required to create the first user after setup
  settings.sso_create_user = Some(true);
  settings_to_db.sso_create_user = Some(true);

  if let Some(oidc_settings) = &settings.oidc_settings() {
    state.try_init(oidc_settings).await.status_context(
      StatusCode::NOT_ACCEPTABLE,
      "Failed to initialize OIDC state",
    )?;
  } else {
    state.deactivate().await;
  }

  db.settings().save_settings(&settings_to_db).await?;

  Ok(())
}
