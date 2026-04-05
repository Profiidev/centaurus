use aide::axum::ApiRouter;
use aide::axum::routing::{get_with, post_with};
use argon2::password_hash::SaltString;
use axum::Json;
use axum_extra::extract::CookieJar;
use rsa::rand_core::OsRng;
use schemars::JsonSchema;
use sea_orm::ConnectionTrait;
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

use crate::backend::auth::jwt_state::JwtState;
use crate::backend::auth::pw_state::PasswordState;
use crate::bail;
use crate::db::init::Connection;
use crate::db::tables::ConnectionExt;
use crate::error::Result;

pub fn router() -> ApiRouter {
  ApiRouter::new()
    .api_route("/", post_with(complete_setup, |op| op.id("completeSetup")))
    .api_route("/", get_with(is_setup, |op| op.id("isSetup")))
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
    .create_user(payload.admin_username, payload.admin_email, hash, salt)
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
}

async fn is_setup(db: Connection) -> Result<Json<IsSetupResponse>> {
  let db_backend = match db.0.get_database_backend() {
    sea_orm::DatabaseBackend::Postgres => "PostgreSQL",
    sea_orm::DatabaseBackend::MySql => "MySQL",
    sea_orm::DatabaseBackend::Sqlite => "SQLite",
  }
  .to_string();

  Ok(Json(IsSetupResponse {
    is_setup: db.setup().is_setup().await?,
    db_backend,
  }))
}
