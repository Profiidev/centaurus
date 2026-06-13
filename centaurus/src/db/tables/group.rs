use sea_orm::{IntoActiveModel, JoinType, QuerySelect, Set, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{
  db::{
    entities::{group, group_permission, group_user, user},
    tables::user::SimpleGroupInfo,
  },
  error::Result,
};

pub struct GroupTable<'db> {
  db: &'db DatabaseConnection,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema))]
pub struct GroupInfo {
  pub id: Uuid,
  pub name: String,
  pub permissions: Vec<String>,
  pub users: Vec<SimpleUserInfo>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema))]
pub struct GroupDetails {
  pub id: Uuid,
  pub name: String,
  pub permissions: Vec<String>,
  pub users: Vec<SimpleUserInfo>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema))]
pub struct SimpleUserInfo {
  pub id: Uuid,
  pub name: String,
}

impl<'db> GroupTable<'db> {
  pub fn new(db: &'db DatabaseConnection) -> Self {
    Self { db }
  }

  pub async fn create_group(&self, name: String) -> Result<Uuid> {
    let group_id = Uuid::new_v4();
    let model = group::Model { id: group_id, name }.into_active_model();

    model.insert(self.db).await?;

    Ok(group_id)
  }

  pub async fn add_permissions_to_group(
    &self,
    group_id: Uuid,
    permissions: Vec<String>,
  ) -> Result<()> {
    let mut models = Vec::new();

    for permission in permissions {
      let model = group_permission::Model {
        group_id,
        permission,
      }
      .into_active_model();
      models.push(model);
    }

    if models.is_empty() {
      return Ok(());
    }

    group_permission::Entity::insert_many(models)
      .exec(self.db)
      .await?;

    Ok(())
  }

  pub async fn get_group_permissions(&self, group_id: Uuid) -> Result<Vec<String>> {
    let permissions = group_permission::Entity::find()
      .filter(group_permission::Column::GroupId.eq(group_id))
      .all(self.db)
      .await?
      .into_iter()
      .map(|gp| gp.permission)
      .collect();

    Ok(permissions)
  }

  pub async fn get_group_users(&self, group_id: Uuid) -> Result<Vec<SimpleUserInfo>> {
    let users = group_user::Entity::find()
      .filter(group_user::Column::GroupId.eq(group_id))
      .find_also_related(user::Entity)
      .all(self.db)
      .await?
      .into_iter()
      .filter_map(|(_, user)| {
        user.map(|u| SimpleUserInfo {
          id: u.id,
          name: u.name,
        })
      })
      .collect();

    Ok(users)
  }

  pub async fn get_group_users_ids(&self, group_id: Uuid) -> Result<Vec<Uuid>> {
    let user_ids = group_user::Entity::find()
      .filter(group_user::Column::GroupId.eq(group_id))
      .all(self.db)
      .await?
      .into_iter()
      .map(|gu| gu.user_id)
      .collect();

    Ok(user_ids)
  }

  pub async fn add_user_to_groups(&self, user_id: Uuid, group_ids: Vec<Uuid>) -> Result<()> {
    let mut models = Vec::new();

    for group_id in group_ids {
      let model = group_user::Model { user_id, group_id }.into_active_model();
      models.push(model);
    }

    if models.is_empty() {
      return Ok(());
    }

    group_user::Entity::insert_many(models)
      .exec(self.db)
      .await?;

    Ok(())
  }

  pub async fn add_users_to_group(&self, group_id: Uuid, user_ids: Vec<Uuid>) -> Result<()> {
    let mut models = Vec::new();

    for user_id in user_ids {
      let model = group_user::Model { user_id, group_id }.into_active_model();
      models.push(model);
    }

    if models.is_empty() {
      return Ok(());
    }

    group_user::Entity::insert_many(models)
      .exec(self.db)
      .await?;

    Ok(())
  }

  pub async fn user_hash_permissions(&self, user_id: Uuid, permission: &str) -> Result<bool> {
    let res = group_user::Entity::find()
      .join(JoinType::InnerJoin, group_user::Relation::Group.def())
      .join(JoinType::InnerJoin, group::Relation::GroupPermission.def())
      .filter(group_user::Column::UserId.eq(user_id))
      .filter(group_permission::Column::Permission.eq(permission))
      .all(self.db)
      .await?;

    Ok(!res.is_empty())
  }

  pub async fn get_user_permissions(&self, user_id: Uuid) -> Result<Vec<String>> {
    let group_permissions = group_permission::Entity::find()
      .join(JoinType::InnerJoin, group_permission::Relation::Group.def())
      .join(JoinType::InnerJoin, group::Relation::GroupUser.def())
      .filter(group_user::Column::UserId.eq(user_id))
      .all(self.db)
      .await?;

    let permissions = group_permissions
      .into_iter()
      .map(|gp| gp.permission)
      .collect();

    Ok(permissions)
  }

  pub async fn list_groups(&self) -> Result<Vec<GroupInfo>> {
    let groups = group::Entity::find().all(self.db).await?;
    let group_user = groups
      .load_many_to_many(user::Entity, group_user::Entity, self.db)
      .await?;
    let group_permissions = groups.load_many(group_permission::Entity, self.db).await?;

    let result = groups
      .into_iter()
      .zip(group_user)
      .zip(group_permissions)
      .map(|((group, users), permissions)| GroupInfo {
        id: group.id,
        name: group.name,
        users: users
          .into_iter()
          .map(|user| SimpleUserInfo {
            id: user.id,
            name: user.name,
          })
          .collect(),
        permissions: permissions.into_iter().map(|gp| gp.permission).collect(),
      })
      .collect();

    Ok(result)
  }

  pub async fn group_info(&self, group_id: Uuid) -> Result<Option<GroupDetails>> {
    let group = group::Entity::find_by_id(group_id).one(self.db).await?;
    let Some(group) = group else {
      return Ok(None);
    };

    let permissions = self.get_group_permissions(group_id).await?;
    let users = self.get_group_users(group_id).await?;

    Ok(Some(GroupDetails {
      id: group.id,
      name: group.name,
      permissions,
      users,
    }))
  }

  pub async fn delete_group(&self, group_id: Uuid) -> Result<group::Model> {
    let group = group::Entity::find_by_id(group_id)
      .one(self.db)
      .await?
      .ok_or_else(|| DbErr::RecordNotFound("Group not found".to_string()))?;

    group::Entity::delete_by_id(group_id).exec(self.db).await?;

    Ok(group)
  }

  pub async fn find_group_by_name(&self, name: &str) -> Result<Option<Uuid>> {
    let group = group::Entity::find()
      .filter(group::Column::Name.eq(name))
      .one(self.db)
      .await?;

    Ok(group.map(|g| g.id))
  }

  pub async fn edit_group(
    &self,
    uuid: Uuid,
    name: String,
    permissions: Vec<String>,
    users: Vec<Uuid>,
  ) -> Result<()> {
    // Update group name
    let mut group_model = group::Entity::find_by_id(uuid)
      .one(self.db)
      .await?
      .ok_or(DbErr::RecordNotFound("Group not found".to_string()))?
      .into_active_model();
    group_model.name = Set(name);
    group_model.update(self.db).await?;

    // Clear existing permissions and users
    group_permission::Entity::delete_many()
      .filter(group_permission::Column::GroupId.eq(uuid))
      .exec(self.db)
      .await?;
    group_user::Entity::delete_many()
      .filter(group_user::Column::GroupId.eq(uuid))
      .exec(self.db)
      .await?;

    // Add new permissions and users
    if !permissions.is_empty() {
      self.add_permissions_to_group(uuid, permissions).await?;
    }
    if !users.is_empty() {
      self.add_users_to_group(uuid, users).await?;
    }

    Ok(())
  }

  pub async fn list_groups_simple(&self) -> Result<Vec<SimpleGroupInfo>> {
    let groups = group::Entity::find()
      .all(self.db)
      .await?
      .into_iter()
      .map(|g| SimpleGroupInfo {
        uuid: g.id,
        name: g.name,
      })
      .collect();

    Ok(groups)
  }

  pub async fn is_last_admin(&self, admin_group: Uuid, user_id: Uuid) -> Result<bool> {
    let admin_users = group_user::Entity::find()
      .filter(group_user::Column::GroupId.eq(admin_group))
      .all(self.db)
      .await?;

    if admin_users.len() == 1 && admin_users[0].user_id == user_id {
      Ok(true)
    } else {
      Ok(false)
    }
  }

  pub async fn is_in_group(&self, admin_group: Uuid, user_id: Uuid) -> Result<bool> {
    let admin_users = group_user::Entity::find()
      .filter(group_user::Column::GroupId.eq(admin_group))
      .filter(group_user::Column::UserId.eq(user_id))
      .all(self.db)
      .await?;

    Ok(admin_users.iter().any(|au| au.user_id == user_id))
  }

  pub async fn get_groups_permissions(&self, group_ids: Vec<Uuid>) -> Result<Vec<String>> {
    let group_permissions = group_permission::Entity::find()
      .filter(group_permission::Column::GroupId.is_in(group_ids))
      .all(self.db)
      .await?;

    let permissions = group_permissions
      .into_iter()
      .map(|gp| gp.permission)
      .collect();

    Ok(permissions)
  }

  pub async fn group_ids(&self, group_names: &[String]) -> Result<Vec<Uuid>> {
    let groups = group::Entity::find()
      .filter(group::Column::Name.is_in(group_names))
      .column(group::Column::Id)
      .into_tuple()
      .all(self.db)
      .await?;

    Ok(groups)
  }

  pub async fn update_name(&self, group_id: Uuid, new_name: String) -> Result<()> {
    let mut group_model = group::Entity::find_by_id(group_id)
      .one(self.db)
      .await?
      .ok_or_else(|| DbErr::RecordNotFound("Group not found".to_string()))?
      .into_active_model();

    group_model.name = Set(new_name);
    group_model.update(self.db).await?;

    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::config::DBConfig;
  use crate::db::init::{Connection, connect_db};
  use crate::db::migrations::Migrator;
  use crate::db::tables::user::UserTable;
  use sea_orm_migration::MigratorTrait;

  async fn setup() -> Connection {
    let db_config = DBConfig::default();
    let conn = connect_db(&db_config, "sqlite::memory:").await;
    Migrator::up(&*conn, None).await.unwrap();
    conn
  }

  async fn make_user(conn: &Connection, name: &str) -> Uuid {
    UserTable::new(conn)
      .create_user(
        name.into(),
        format!("{name}@example.com"),
        "pass".into(),
        "salt".into(),
        false,
      )
      .await
      .unwrap()
  }

  #[tokio::test]
  async fn test_group_table() {
    let conn = setup().await;

    let table = GroupTable::new(&conn);
    let id = table.create_group("admin".into()).await.unwrap();
    table
      .add_permissions_to_group(id, vec!["read".into(), "write".into()])
      .await
      .unwrap();

    let perms = table.get_group_permissions(id).await.unwrap();
    assert_eq!(perms.len(), 2);
    assert!(perms.contains(&"read".into()));
  }

  #[tokio::test]
  async fn test_add_permissions_empty_is_noop() {
    let conn = setup().await;
    let table = GroupTable::new(&conn);
    let id = table.create_group("g".into()).await.unwrap();

    table.add_permissions_to_group(id, vec![]).await.unwrap();
    assert!(table.get_group_permissions(id).await.unwrap().is_empty());

    // Empty user/group additions must also be no-ops and not error.
    table.add_users_to_group(id, vec![]).await.unwrap();
    table
      .add_user_to_groups(Uuid::new_v4(), vec![])
      .await
      .unwrap();
  }

  #[tokio::test]
  async fn test_group_users_membership() {
    let conn = setup().await;
    let table = GroupTable::new(&conn);
    let group = table.create_group("team".into()).await.unwrap();
    let user_a = make_user(&conn, "a").await;
    let user_b = make_user(&conn, "b").await;

    table
      .add_users_to_group(group, vec![user_a, user_b])
      .await
      .unwrap();

    // Membership queries should be consistent across the different accessors.
    let users = table.get_group_users(group).await.unwrap();
    assert_eq!(users.len(), 2);
    let ids = table.get_group_users_ids(group).await.unwrap();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&user_a) && ids.contains(&user_b));

    assert!(table.is_in_group(group, user_a).await.unwrap());
    assert!(!table.is_in_group(group, Uuid::new_v4()).await.unwrap());
  }

  #[tokio::test]
  async fn test_permission_invariants() {
    let conn = setup().await;
    let table = GroupTable::new(&conn);
    let group = table.create_group("admins".into()).await.unwrap();
    let user = make_user(&conn, "u").await;
    table.add_users_to_group(group, vec![user]).await.unwrap();
    table
      .add_permissions_to_group(group, vec!["user:edit".into(), "user:view".into()])
      .await
      .unwrap();

    // A user's effective permissions equal the union of their groups' permissions.
    let perms = table.get_user_permissions(user).await.unwrap();
    assert_eq!(perms.len(), 2);
    assert!(
      table
        .user_hash_permissions(user, "user:edit")
        .await
        .unwrap()
    );
    assert!(
      !table
        .user_hash_permissions(user, "user:delete")
        .await
        .unwrap()
    );

    // A user with no groups has no permissions.
    let lonely = make_user(&conn, "lonely").await;
    assert!(table.get_user_permissions(lonely).await.unwrap().is_empty());
    assert!(
      !table
        .user_hash_permissions(lonely, "user:edit")
        .await
        .unwrap()
    );

    let group_perms = table.get_groups_permissions(vec![group]).await.unwrap();
    assert_eq!(group_perms.len(), 2);
  }

  #[tokio::test]
  async fn test_find_and_ids() {
    let conn = setup().await;
    let table = GroupTable::new(&conn);
    let id = table.create_group("findme".into()).await.unwrap();

    assert_eq!(table.find_group_by_name("findme").await.unwrap(), Some(id));
    assert_eq!(table.find_group_by_name("missing").await.unwrap(), None);

    table.create_group("other".into()).await.unwrap();
    let ids = table
      .group_ids(&["findme".into(), "other".into(), "nope".into()])
      .await
      .unwrap();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&id));
  }

  #[tokio::test]
  async fn test_list_and_info() {
    let conn = setup().await;
    let table = GroupTable::new(&conn);
    let group = table.create_group("g1".into()).await.unwrap();
    let user = make_user(&conn, "m").await;
    table.add_users_to_group(group, vec![user]).await.unwrap();
    table
      .add_permissions_to_group(group, vec!["p1".into()])
      .await
      .unwrap();

    let list = table.list_groups().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].users.len(), 1);
    assert_eq!(list[0].permissions, vec!["p1".to_string()]);

    let simple = table.list_groups_simple().await.unwrap();
    assert_eq!(simple.len(), 1);
    assert_eq!(simple[0].name, "g1");

    let info = table.group_info(group).await.unwrap().unwrap();
    assert_eq!(info.name, "g1");
    assert_eq!(info.users.len(), 1);
    assert_eq!(info.permissions, vec!["p1".to_string()]);

    // group_info for a missing id resolves to None rather than erroring.
    assert!(table.group_info(Uuid::new_v4()).await.unwrap().is_none());
  }

  #[tokio::test]
  async fn test_edit_group_replaces_state() {
    let conn = setup().await;
    let table = GroupTable::new(&conn);
    let group = table.create_group("old".into()).await.unwrap();
    let user_a = make_user(&conn, "ea").await;
    let user_b = make_user(&conn, "eb").await;

    table.add_users_to_group(group, vec![user_a]).await.unwrap();
    table
      .add_permissions_to_group(group, vec!["old_perm".into()])
      .await
      .unwrap();

    table
      .edit_group(group, "new".into(), vec!["new_perm".into()], vec![user_b])
      .await
      .unwrap();

    // edit_group fully replaces name, permissions, and membership.
    let info = table.group_info(group).await.unwrap().unwrap();
    assert_eq!(info.name, "new");
    assert_eq!(info.permissions, vec!["new_perm".to_string()]);
    assert_eq!(info.users.len(), 1);
    assert_eq!(info.users[0].id, user_b);

    table.update_name(group, "renamed".into()).await.unwrap();
    assert_eq!(
      table.group_info(group).await.unwrap().unwrap().name,
      "renamed"
    );
  }

  #[tokio::test]
  async fn test_delete_group() {
    let conn = setup().await;
    let table = GroupTable::new(&conn);
    let group = table.create_group("doomed".into()).await.unwrap();

    let deleted = table.delete_group(group).await.unwrap();
    assert_eq!(deleted.name, "doomed");
    assert!(table.group_info(group).await.unwrap().is_none());

    // Deleting a non-existent group is an error.
    assert!(table.delete_group(Uuid::new_v4()).await.is_err());
  }

  #[tokio::test]
  async fn test_is_last_admin() {
    let conn = setup().await;
    let table = GroupTable::new(&conn);
    let admin = table.create_group("admin".into()).await.unwrap();
    let user_a = make_user(&conn, "la").await;
    let user_b = make_user(&conn, "lb").await;

    table.add_users_to_group(admin, vec![user_a]).await.unwrap();
    // Sole member is the last admin.
    assert!(table.is_last_admin(admin, user_a).await.unwrap());
    // A non-member is never the last admin.
    assert!(!table.is_last_admin(admin, user_b).await.unwrap());

    table.add_users_to_group(admin, vec![user_b]).await.unwrap();
    // With two members, no single user is the last admin.
    assert!(!table.is_last_admin(admin, user_a).await.unwrap());
  }

  #[tokio::test]
  async fn test_edit_missing_group_errors() {
    let conn = setup().await;
    let table = GroupTable::new(&conn);
    assert!(
      table
        .edit_group(Uuid::new_v4(), "x".into(), vec![], vec![])
        .await
        .is_err()
    );
    assert!(table.update_name(Uuid::new_v4(), "x".into()).await.is_err());
  }
}
