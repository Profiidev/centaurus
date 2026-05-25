use std::{sync::Arc, time::Instant};

use aide::{
  OperationIo,
  axum::routing::{ApiMethodRouter, post_with},
};
use axum::{Extension, Json, extract::FromRequestParts};
use dashmap::DashMap;
use rand::{RngExt, distr::Uniform};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::spawn;
use uuid::Uuid;

use crate::{
  backend::{
    auth::{jwt_auth::JwtAuth, permission::UserEdit},
    config::SiteConfig,
    endpoints::{
      mail::template::confirm_code,
      websocket::state::{UpdateMessage, Updater},
    },
  },
  bail,
  db::{init::Connection, tables::ConnectionExt},
  error::Result,
  mail::Mailer,
};

pub fn change_email_route<T: UpdateMessage>() -> ApiMethodRouter<()> {
  post_with(change_user_email::<T>, |op| op.id("changeUserEmail"))
}

pub fn start_email_change_route() -> ApiMethodRouter<()> {
  post_with(start_email_change, |op| op.id("startEmailChange"))
}

pub fn confirm_email_change_route<T: UpdateMessage>() -> ApiMethodRouter<()> {
  post_with(confirm_email_change::<T>, |op| op.id("confirmEmailChange"))
}

pub fn gen_code() -> String {
  // unwrap is safe because the range is valid
  rand::rng()
    .sample_iter(Uniform::new(48, 58).unwrap())
    .take(6)
    .map(char::from)
    .collect()
}

#[derive(Deserialize, JsonSchema)]
struct ChangeUserEmail {
  uuid: Uuid,
  new_email: String,
}

async fn change_user_email<T: UpdateMessage>(
  auth: JwtAuth<UserEdit>,
  db: Connection,
  mailer: Mailer,
  updater: Updater<T>,
  Json(req): Json<ChangeUserEmail>,
) -> Result<()> {
  if mailer.is_active().await {
    bail!(
      BAD_REQUEST,
      "Cannot change email when mail service is active"
    );
  }

  if req.new_email.is_empty() {
    bail!(BAD_REQUEST, "New email cannot be empty");
  }

  let self_permissions = db.group().get_user_permissions(auth.user_id).await?;
  let target_permissions = db.group().get_user_permissions(req.uuid).await?;

  if target_permissions
    .iter()
    .any(|p| !self_permissions.contains(p))
  {
    bail!(
      FORBIDDEN,
      "Cannot change email for a user with higher permissions"
    );
  }

  if db.user().get_user_by_email(&req.new_email).await.is_ok() {
    bail!(CONFLICT, "Email is already in use");
  }

  db.user().change_email(req.uuid, req.new_email).await?;
  updater.broadcast(T::user(req.uuid)).await;

  Ok(())
}

struct ChangeInfo {
  new_email: String,
  new_code: String,
  old_code: String,
  created: Instant,
}

#[derive(Clone, FromRequestParts, OperationIo)]
#[from_request(via(Extension))]
pub struct EmailChangeState {
  changes: Arc<DashMap<Uuid, ChangeInfo>>,
}

impl EmailChangeState {
  pub fn init() -> Self {
    let changes: Arc<DashMap<Uuid, ChangeInfo>> = Arc::new(DashMap::new());

    spawn({
      let changes = Arc::clone(&changes);

      async move {
        loop {
          let now = Instant::now();
          changes.retain(|_, data| now.duration_since(data.created).as_secs() < 600);
          tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        }
      }
    });

    Self { changes }
  }
}

#[derive(Deserialize, JsonSchema)]
struct EmailChange {
  new_email: String,
}

async fn start_email_change(
  auth: JwtAuth,
  db: Connection,
  mail: Mailer,
  state: EmailChangeState,
  config: SiteConfig,
  Json(req): Json<EmailChange>,
) -> Result<()> {
  if db.user().get_user_by_email(&req.new_email).await.is_ok() {
    bail!(CONFLICT, "A user with this email already exists");
  }

  let change = ChangeInfo {
    new_email: req.new_email.clone(),
    new_code: gen_code(),
    old_code: gen_code(),
    created: Instant::now(),
  };

  let user = db.user().get_user_by_id(auth.user_id).await?;
  if user.oidc_user {
    bail!(BAD_REQUEST, "Cannot change email for an OIDC user");
  }

  mail
    .send_mail(
      user.name.clone(),
      user.email,
      "Email Change Request".into(),
      confirm_code(&change.old_code, true, config.site_url.as_str()),
    )
    .await?;

  mail
    .send_mail(
      user.name,
      req.new_email,
      "Email Change Confirmation".into(),
      confirm_code(&change.new_code, false, config.site_url.as_str()),
    )
    .await?;

  state.changes.insert(auth.user_id, change);

  Ok(())
}

#[derive(Deserialize, JsonSchema)]
struct EmailChangeConfirm {
  new_code: String,
  old_code: String,
}

async fn confirm_email_change<T: UpdateMessage>(
  auth: JwtAuth,
  db: Connection,
  state: EmailChangeState,
  updater: Updater<T>,
  Json(req): Json<EmailChangeConfirm>,
) -> Result<()> {
  let Some(change) = state.changes.get(&auth.user_id) else {
    bail!(NOT_FOUND, "No email change request found");
  };

  if change.old_code != req.old_code {
    bail!(FORBIDDEN, "Invalid confirmation code for current email");
  }

  if change.new_code != req.new_code {
    bail!(UNAUTHORIZED, "Invalid confirmation code for new email");
  }

  db.user()
    .change_email(auth.user_id, change.new_email.clone())
    .await?;

  drop(change);
  state.changes.remove(&auth.user_id);

  updater.broadcast(T::user(auth.user_id)).await;

  Ok(())
}
