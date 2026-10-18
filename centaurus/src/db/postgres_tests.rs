//! Postgres-specific behavior tested against a real Postgres container (requires Docker).

use std::sync::{Arc, atomic::AtomicI32};

use chrono::{Duration, Utc};
use sea_orm::{
  ConnectionTrait, DatabaseBackend, EntityTrait, PaginatorTrait, Statement, prelude::Uuid,
};
use sea_orm_migration::MigratorTrait;
use testcontainers::{
  ContainerAsync, GenericImage, ImageExt,
  core::{IntoContainerPort, WaitFor},
  runners::AsyncRunner,
};

use crate::db::{
  config::DBConfig,
  entities::{group_permission, group_user},
  init::{Connection, init_db},
  migrations::Migrator,
  tables::ConnectionExt,
};

/// Starts a Postgres container and returns it with a fully migrated connection.
/// The container is removed when the returned handle is dropped.
pub(crate) async fn postgres() -> (ContainerAsync<GenericImage>, Connection) {
  let container = GenericImage::new("postgres", "18-alpine")
    .with_exposed_port(5432.tcp())
    // The init script runs a temporary server first, the real one logs to stdout
    .with_wait_for(WaitFor::message_on_stderr(
      "database system is ready to accept connections",
    ))
    .with_wait_for(WaitFor::message_on_stdout(
      "database system is ready to accept connections",
    ))
    .with_env_var("POSTGRES_PASSWORD", "postgres")
    .with_cmd(["postgres", "-c", "fsync=off"])
    .start()
    .await
    .expect("Failed to start postgres container (is Docker running?)");

  let conn = init_db::<Migrator>(&DBConfig::default(), &url(&container).await).await;
  (container, conn)
}

async fn url(container: &ContainerAsync<GenericImage>) -> String {
  let host = container.get_host().await.unwrap();
  let port = container.get_host_port_ipv4(5432).await.unwrap();
  format!("postgres://postgres:postgres@{host}:{port}/postgres")
}

async fn make_user(conn: &Connection, name: &str, oidc_subject: Option<&str>) -> Result<Uuid, ()> {
  conn
    .user()
    .create_user(
      name.into(),
      format!("{name}@example.com"),
      "pass".into(),
      "salt".into(),
      oidc_subject.is_some(),
      oidc_subject.map(String::from),
    )
    .await
    .map_err(|_| ())
}

async fn applied_versions(conn: &Connection) -> Vec<String> {
  conn
    .query_all_raw(Statement::from_string(
      conn.get_database_backend(),
      "SELECT version FROM seaql_migrations ORDER BY version",
    ))
    .await
    .unwrap()
    .into_iter()
    .map(|row| row.try_get_by_index::<String>(0).unwrap())
    .collect()
}

#[tokio::test]
async fn migrations_up_down_up() {
  let (_pg, conn) = postgres().await;
  assert_eq!(conn.get_database_backend(), DatabaseBackend::Postgres);
  assert_eq!(
    applied_versions(&conn).await.len(),
    Migrator::migrations().len()
  );

  Migrator::down(&*conn, None).await.unwrap();
  assert!(applied_versions(&conn).await.is_empty());

  Migrator::up(&*conn, None).await.unwrap();
  assert_eq!(
    applied_versions(&conn).await.len(),
    Migrator::migrations().len()
  );
  assert_eq!(conn.user().count_users().await.unwrap(), 0);
}

#[cfg(feature = "fix-migrations")]
#[tokio::test]
async fn fix_migrations_renames_legacy_versions() {
  let (pg, conn) = postgres().await;
  let expected = applied_versions(&conn).await;

  // Rewrite the history to the pre-centaurus migration names
  conn
    .execute_unprepared(
      "
      UPDATE seaql_migrations SET version = CASE
        WHEN version = 'm0_key'         THEN 'm20250301_215149_create_key_table'
        WHEN version = 'm1_invalid_jwt' THEN 'm20260123_144736_invalid_jwt'
        WHEN version = 'm2_settings'    THEN 'm20260127_211643_settings'
        WHEN version = 'm3_user'        THEN 'm20260123_144752_user'
        WHEN version = 'm4_groups'      THEN 'm20260126_155842_group'
        WHEN version = 'm5_setup'       THEN 'm20260126_160754_setup'
        ELSE version
      END;
      INSERT INTO seaql_migrations (version, applied_at) VALUES ('m20260129_154755_user_avatar', 0);
      ",
    )
    .await
    .unwrap();

  // init_db renames the legacy entries, so the migrator sees everything as applied
  let conn = init_db::<Migrator>(&DBConfig::default(), &url(&pg).await).await;

  assert_eq!(applied_versions(&conn).await, expected);
}

#[tokio::test]
async fn user_roundtrip_and_unique_email() {
  let (_pg, conn) = postgres().await;

  let id = make_user(&conn, "alice", None).await.unwrap();
  let user = conn.user().get_user_by_id(id).await.unwrap();
  assert_eq!(user.id, id);
  assert_eq!(user.email, "alice@example.com");
  assert_eq!(user.oidc_subject, None);

  // Unique index on email is enforced
  assert!(make_user(&conn, "alice", None).await.is_err());
  assert_eq!(conn.user().count_users().await.unwrap(), 1);

  conn
    .user()
    .update_user_name(id, "bob".into())
    .await
    .unwrap();
  conn
    .user()
    .change_email(id, "Bob@Example.com".into())
    .await
    .unwrap();
  let user = conn
    .user()
    .get_user_by_email("bob@example.com")
    .await
    .unwrap();
  assert_eq!(user.name, "bob");
}

#[tokio::test]
async fn oidc_subject_unique_and_nullable() {
  let (_pg, conn) = postgres().await;

  // NULL subjects never collide with each other in a unique index
  make_user(&conn, "a", None).await.unwrap();
  let b = make_user(&conn, "b", None).await.unwrap();

  make_user(&conn, "c", Some("sub-1")).await.unwrap();
  assert!(make_user(&conn, "d", Some("sub-1")).await.is_err());

  // Links an existing user without subject by email
  let linked = conn
    .user()
    .resolve_oidc_user("sub-2", "b@example.com")
    .await
    .unwrap()
    .unwrap();
  assert_eq!(linked.id, b);
  assert_eq!(linked.oidc_subject.as_deref(), Some("sub-2"));

  // Setting an already used subject on another user violates the index
  assert!(
    conn
      .user()
      .set_oidc_subject(b, "sub-1".into())
      .await
      .is_err()
  );
}

#[tokio::test]
async fn fk_cascade_on_delete() {
  let (_pg, conn) = postgres().await;
  let user = make_user(&conn, "u", None).await.unwrap();
  let group = conn.group().create_group("g".into()).await.unwrap();
  conn
    .group()
    .add_permissions_to_group(group, vec!["p".into()])
    .await
    .unwrap();
  conn
    .group()
    .add_user_to_groups(user, vec![group])
    .await
    .unwrap();

  #[cfg(feature = "avatar")]
  {
    let data = vec![0u8, 1, 2, 255];
    conn
      .user()
      .update_user_avatar(user, data.clone())
      .await
      .unwrap();
    assert_eq!(conn.user().get_user_avatar(user).await.unwrap(), Some(data));
    conn.user().update_user_avatar(user, vec![9]).await.unwrap();
    assert_eq!(
      conn.user().get_user_avatar(user).await.unwrap(),
      Some(vec![9])
    );
  }

  // Unknown user ids are rejected by the foreign key
  assert!(
    conn
      .group()
      .add_user_to_groups(Uuid::now_v7(), vec![group])
      .await
      .is_err()
  );

  // Deleting the user cascades to memberships and the avatar
  conn.user().delete_user(user).await.unwrap();
  assert_eq!(group_user::Entity::find().count(&*conn).await.unwrap(), 0);
  #[cfg(feature = "avatar")]
  assert_eq!(conn.user().get_user_avatar(user).await.unwrap(), None);

  // Deleting the group cascades to its permissions and memberships
  let user = make_user(&conn, "v", None).await.unwrap();
  conn
    .group()
    .add_user_to_groups(user, vec![group])
    .await
    .unwrap();
  conn.group().delete_group(group).await.unwrap();
  assert_eq!(group_user::Entity::find().count(&*conn).await.unwrap(), 0);
  assert_eq!(
    group_permission::Entity::find()
      .count(&*conn)
      .await
      .unwrap(),
    0
  );
}

#[tokio::test]
async fn group_queries() {
  let (_pg, conn) = postgres().await;
  let alice = make_user(&conn, "alice", None).await.unwrap();
  let bob = make_user(&conn, "bob", None).await.unwrap();

  let admin = conn.group().create_group("admin".into()).await.unwrap();
  let readers = conn.group().create_group("readers".into()).await.unwrap();
  conn
    .group()
    .add_permissions_to_group(admin, vec!["write".into(), "read".into()])
    .await
    .unwrap();
  conn
    .group()
    .add_permissions_to_group(readers, vec!["read".into()])
    .await
    .unwrap();
  conn
    .group()
    .add_user_to_groups(alice, vec![admin])
    .await
    .unwrap();
  conn
    .group()
    .add_users_to_group(readers, vec![alice, bob])
    .await
    .unwrap();

  // Composite primary keys reject duplicates
  assert!(
    conn
      .group()
      .add_permissions_to_group(readers, vec!["read".into()])
      .await
      .is_err()
  );
  assert!(
    conn
      .group()
      .add_users_to_group(readers, vec![bob])
      .await
      .is_err()
  );

  assert!(
    conn
      .group()
      .user_hash_permissions(alice, "write")
      .await
      .unwrap()
  );
  assert!(
    !conn
      .group()
      .user_hash_permissions(bob, "write")
      .await
      .unwrap()
  );
  let mut perms = conn.group().get_user_permissions(alice).await.unwrap();
  perms.sort();
  assert_eq!(perms, ["read", "read", "write"]);

  let mut groups = conn.group().list_groups().await.unwrap();
  groups.sort_by(|a, b| a.name.cmp(&b.name));
  assert_eq!(groups[0].users.len(), 1);
  assert_eq!(groups[1].users.len(), 2);

  let mut users = conn.user().list_users().await.unwrap();
  users.sort_by(|a, b| a.name.cmp(&b.name));
  assert_eq!(users[0].groups.len(), 2);
  assert_eq!(users[1].groups.len(), 1);

  let ids = conn
    .group()
    .group_ids(&["admin".into(), "missing".into()])
    .await
    .unwrap();
  assert_eq!(ids, [admin]);
  assert_eq!(
    conn
      .group()
      .get_groups_permissions(vec![admin, readers])
      .await
      .unwrap()
      .len(),
    3
  );

  assert!(conn.group().is_last_admin(admin, alice).await.unwrap());
  assert!(conn.group().is_in_group(readers, bob).await.unwrap());

  conn
    .group()
    .edit_group(readers, "viewers".into(), vec!["view".into()], vec![bob])
    .await
    .unwrap();
  let details = conn.group().group_info(readers).await.unwrap().unwrap();
  assert_eq!(details.name, "viewers");
  assert_eq!(details.permissions, ["view"]);
  assert_eq!(details.users.len(), 1);

  let info = conn.user().user_info(bob).await.unwrap().unwrap();
  assert_eq!(info.permissions, ["view"]);
  conn
    .user()
    .edit_user(bob, "robert".into(), vec![])
    .await
    .unwrap();
  assert!(conn.user().get_user_groups(bob).await.unwrap().is_empty());
}

#[tokio::test]
async fn invalid_jwt_timestamp_compare() {
  let (_pg, conn) = postgres().await;
  let count = Arc::new(AtomicI32::new(0));
  let table = conn.invalid_jwt();

  table
    .invalidate_jwt("old".into(), Utc::now() - Duration::hours(1), count.clone())
    .await
    .unwrap();
  table
    .invalidate_jwt("new".into(), Utc::now() + Duration::hours(1), count.clone())
    .await
    .unwrap();

  table.remove_expired().await.unwrap();
  assert!(table.is_token_valid("old").await.unwrap());
  assert!(!table.is_token_valid("new").await.unwrap());
}

#[tokio::test]
async fn settings_setup_and_keys() {
  use crate::db::{entities::setup, settings::Settings};

  #[derive(
    serde::Serialize, serde::Deserialize, Default, PartialEq, Debug, schemars::JsonSchema,
  )]
  struct TestSettings {
    val: String,
  }
  impl Settings for TestSettings {
    fn id() -> i32 {
      99
    }
  }

  let (_pg, conn) = postgres().await;

  // Insert then update path of the upsert
  for val in ["first", "second"] {
    conn
      .settings()
      .save_settings(&TestSettings { val: val.into() })
      .await
      .unwrap();
  }
  let s: TestSettings = conn.settings().get_settings().await.unwrap();
  assert_eq!(s.val, "second");

  assert!(!conn.setup().is_setup().await.unwrap());
  let group = Uuid::now_v7();
  conn.setup().set_admin_group_created(group).await.unwrap();
  conn.setup().mark_completed().await.unwrap();
  assert!(conn.setup().is_setup().await.unwrap());
  assert_eq!(
    conn.setup().get_admin_group_id().await.unwrap(),
    Some(group)
  );
  assert_eq!(setup::Entity::find().count(&*conn).await.unwrap(), 1);

  let id = Uuid::now_v7();
  conn
    .key()
    .create_key("k".into(), "secret".into(), id)
    .await
    .unwrap();
  assert_eq!(conn.key().get_key_by_name("k".into()).await.unwrap().id, id);
  assert!(conn.key().get_key_by_name("missing".into()).await.is_err());
}
