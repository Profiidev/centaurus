use aide::axum::ApiRouter;
use aide::axum::routing::{ApiMethodRouter, delete_with, get_with, post_with, put_with};
use axum::{Json, extract::Path};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::backend::auth::jwt_auth::JwtAuth;
use crate::backend::auth::permission::{GroupEdit, GroupView};
use crate::backend::endpoints::websocket::state::{UpdateMessage, Updater};
use crate::bail;
use crate::db::init::Connection;
use crate::db::tables::ConnectionExt;
use crate::db::tables::group::{GroupDetails, GroupInfo, SimpleUserInfo};
use crate::error::Result;

pub fn router<T: UpdateMessage>() -> ApiRouter {
  ApiRouter::new()
    .api_route("/", list_groups_route())
    .api_route("/", create_group_route::<T>())
    .api_route("/", delete_group_route::<T>())
    .api_route("/", edit_group_route::<T>())
    .api_route("/{uuid}", group_info_route())
    .api_route("/users", list_users_simple_route())
}

#[cfg(feature = "openapi")]
pub fn list_groups_route() -> ApiMethodRouter<()> {
  get_with(list_groups, |op| op.id("listGroups"))
}

pub fn create_group_route<T: UpdateMessage>() -> ApiMethodRouter<()> {
  post_with(create_group::<T>, |op| op.id("createGroup"))
}

pub fn delete_group_route<T: UpdateMessage>() -> ApiMethodRouter<()> {
  delete_with(delete_group::<T>, |op| op.id("deleteGroup"))
}

pub fn edit_group_route<T: UpdateMessage>() -> ApiMethodRouter<()> {
  put_with(edit_group::<T>, |op| op.id("editGroup"))
}

pub fn group_info_route() -> ApiMethodRouter<()> {
  get_with(group_info, |op| op.id("groupInfo"))
}

pub fn list_users_simple_route() -> ApiMethodRouter<()> {
  get_with(list_users_simple, |op| op.id("listUsersSimple"))
}

#[derive(Serialize, JsonSchema)]
struct ListGroupResponse {
  groups: Vec<GroupInfo>,
  admin_group: Option<Uuid>,
}

async fn list_groups(_auth: JwtAuth<GroupView>, db: Connection) -> Result<Json<ListGroupResponse>> {
  let groups = db.group().list_groups().await?;
  let admin_group = db.setup().get_admin_group_id().await?;
  Ok(Json(ListGroupResponse {
    groups,
    admin_group,
  }))
}

#[derive(Deserialize, JsonSchema)]
struct GroupViewPath {
  uuid: Uuid,
}

async fn group_info(
  _auth: JwtAuth<GroupView>,
  db: Connection,
  Path(path): Path<GroupViewPath>,
) -> Result<Json<GroupDetails>> {
  let info = db.group().group_info(path.uuid).await?;
  let Some(info) = info else {
    bail!(NOT_FOUND, "Group not found");
  };
  Ok(Json(info))
}

#[derive(Deserialize, JsonSchema)]
struct CreateGroupRequest {
  name: String,
}

#[derive(Serialize, JsonSchema)]
struct GroupCreateResponse {
  uuid: Uuid,
}

async fn create_group<T: UpdateMessage>(
  _auth: JwtAuth<GroupEdit>,
  db: Connection,
  updater: Updater<T>,
  Json(data): Json<CreateGroupRequest>,
) -> Result<Json<GroupCreateResponse>> {
  if data.name.trim().is_empty() {
    bail!(BAD_REQUEST, "Group name cannot be empty");
  }

  if db.group().find_group_by_name(&data.name).await?.is_some() {
    bail!(CONFLICT, "A group with this name already exists");
  }

  let group_id = db.group().create_group(data.name).await?;
  updater.broadcast(T::group(group_id)).await;

  Ok(Json(GroupCreateResponse { uuid: group_id }))
}

#[derive(Deserialize, JsonSchema)]
struct DeleteGroupRequest {
  uuid: Uuid,
}

async fn delete_group<T: UpdateMessage>(
  _auth: JwtAuth<GroupEdit>,
  db: Connection,
  updater: Updater<T>,
  Json(data): Json<DeleteGroupRequest>,
) -> Result<()> {
  if let Some(admin_group) = db.setup().get_admin_group_id().await?
    && admin_group == data.uuid
  {
    bail!(BAD_REQUEST, "Cannot delete the admin group");
  }

  let users = db.group().get_group_users_ids(data.uuid).await?;
  db.group().delete_group(data.uuid).await?;

  updater.broadcast(T::group(data.uuid)).await;
  for user_id in users {
    updater.send_to(user_id, T::user_permissions()).await;
  }

  Ok(())
}

#[derive(Deserialize, JsonSchema)]
struct EditGroupRequest {
  uuid: Uuid,
  name: String,
  permissions: Vec<String>,
  users: Vec<Uuid>,
}

async fn edit_group<T: UpdateMessage>(
  auth: JwtAuth<GroupEdit>,
  db: Connection,
  updater: Updater<T>,
  Json(data): Json<EditGroupRequest>,
) -> Result<()> {
  if data.name.trim().is_empty() {
    bail!(BAD_REQUEST, "Group name cannot be empty");
  }

  if let Some(admin_group) = db.setup().get_admin_group_id().await?
    && admin_group == data.uuid
  {
    bail!(BAD_REQUEST, "Cannot edit the admin group");
  }

  if let Some(existing_group) = db.group().find_group_by_name(&data.name).await?
    && existing_group != data.uuid
  {
    bail!(CONFLICT, "A group with this name already exists");
  }

  let Some(group) = db.group().group_info(data.uuid).await? else {
    bail!(NOT_FOUND, "Group not found");
  };

  let user_permissions = db.group().get_user_permissions(auth.user_id).await?;
  if group
    .permissions
    .iter()
    .any(|perm| !user_permissions.contains(perm))
  {
    bail!(
      FORBIDDEN,
      "Cannot edit a group with permissions you do not have"
    );
  }
  if data
    .permissions
    .iter()
    .any(|perm| !user_permissions.contains(perm))
  {
    bail!(
      FORBIDDEN,
      "Cannot assign permissions you do not have to a group"
    );
  }

  let old_users = db.group().get_group_users_ids(data.uuid).await?;

  db.group()
    .edit_group(
      data.uuid,
      data.name,
      data.permissions.clone(),
      data.users.clone(),
    )
    .await?;

  updater.broadcast(T::group(data.uuid)).await;

  let permissions_changed = group.permissions.len() != data.permissions.len()
    || group
      .permissions
      .iter()
      .any(|perm| !data.permissions.contains(perm));

  let mut users_to_notify = old_users.clone();
  users_to_notify.extend(data.users.clone());
  users_to_notify.sort_unstable();
  users_to_notify.dedup();

  // Only notify users that where added or removed
  if !permissions_changed {
    users_to_notify.retain(|user_id| {
      let in_old = old_users.contains(user_id);
      let in_new = data.users.contains(user_id);
      in_old != in_new
    });
  }

  for user_id in users_to_notify {
    updater.send_to(user_id, T::user_permissions()).await;
  }

  Ok(())
}

async fn list_users_simple(
  _auth: JwtAuth<GroupView>,
  db: Connection,
) -> Result<Json<Vec<SimpleUserInfo>>> {
  let users = db.user().list_users_simple().await?;
  Ok(Json(users))
}
