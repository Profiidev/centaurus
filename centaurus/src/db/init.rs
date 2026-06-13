use std::{ops::Deref, time::Duration};

use sea_orm::{
  ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement,
};
use sea_orm_migration::MigratorTrait;
use tracing::instrument;

use crate::db::config::DBConfig;
#[cfg(feature = "fix-migrations")]
use crate::{bail, error::Result};

#[derive(Clone)]
#[cfg_attr(feature = "openapi", derive(aide::OperationIo))]
#[cfg_attr(feature = "backend", derive(axum::extract::FromRequestParts))]
#[cfg_attr(feature = "backend", from_request(via(axum::Extension)))]
pub struct Connection(pub DatabaseConnection);

impl Deref for Connection {
  type Target = DatabaseConnection;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

#[instrument(skip(config))]
pub async fn init_db<M: MigratorTrait>(config: &DBConfig, connection_url: &str) -> Connection {
  let conn = create_connection(config, connection_url).await;

  #[cfg(feature = "fix-migrations")]
  migrate_to_centaurus_migrations(&conn)
    .await
    .expect("Failed to migrate to centaurus migrations");

  M::up(&conn, None)
    .await
    .expect("Failed to run database migrations");

  sqlite_init(&conn).await;

  Connection(conn)
}

pub async fn connect_db(config: &DBConfig, connection_url: &str) -> Connection {
  let conn = create_connection(config, connection_url).await;
  sqlite_init(&conn).await;
  Connection(conn)
}

async fn create_connection(config: &DBConfig, connection_url: &str) -> DatabaseConnection {
  let mut options = ConnectOptions::new(connection_url);
  options
    .max_connections(config.database_max_connections)
    .min_connections(config.database_min_connections)
    .connect_timeout(Duration::from_secs(config.database_connect_timeout))
    .sqlx_logging(config.database_logging);

  Database::connect(options)
    .await
    .expect("Failed to connect to database")
}

async fn sqlite_init(conn: &DatabaseConnection) {
  if conn.get_database_backend() == DatabaseBackend::Sqlite {
    conn
      .execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        "PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 60000;".to_string(),
      ))
      .await
      .expect("Failed to set SQLite pragmas");
  }
}

#[cfg(feature = "fix-migrations")]
async fn migrate_to_centaurus_migrations(conn: &DatabaseConnection) -> Result<()> {
  let backend = conn.get_database_backend();
  let stmt = match backend {
    DatabaseBackend::Postgres => Statement::from_string(
      backend,
      "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public';".to_string(),
    ),
    DatabaseBackend::Sqlite => Statement::from_string(
      backend,
      "SELECT name as table_name FROM sqlite_master WHERE type='table';".to_string(),
    ),
    _ => {
      bail!("Unsupported database backend for migration table rename");
    }
  };

  let tables = conn
    .query_all(stmt)
    .await?
    .into_iter()
    .filter_map(|row| row.try_get_by_index::<String>(0).ok())
    .collect::<Vec<String>>();

  if !tables.contains(&"seaql_migrations".to_string()) {
    tracing::info!("No 'seaql_migrations' table found, skipping migration table rename");
    return Ok(());
  }

  let stmt = Statement::from_string(
    backend,
    "
    UPDATE seaql_migrations
    SET version = CASE
        WHEN version = 'm20250301_215149_create_key_table' THEN 'm0_key'
        WHEN version = 'm20260123_144736_invalid_jwt'      THEN 'm1_invalid_jwt'
        WHEN version = 'm20260127_211643_settings'         THEN 'm2_settings'
        WHEN version = 'm20260123_144752_user'             THEN 'm3_user'
        WHEN version = 'm20260126_155842_group'            THEN 'm4_groups'
        WHEN version = 'm20260126_160754_setup'            THEN 'm5_setup'
        ELSE version
    END
    WHERE version LIKE 'm20%';
  ",
  );
  conn.execute(stmt).await?;

  let stmt = Statement::from_string(
    backend,
    "DELETE FROM seaql_migrations WHERE version = 'm20260129_154755_user_avatar';",
  );
  conn.execute(stmt).await?;

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::config::DBConfig;

  #[tokio::test]
  async fn test_connect_db() {
    let config = DBConfig::default();
    let conn = connect_db(&config, "sqlite::memory:").await;
    assert!(conn.get_database_backend() == DatabaseBackend::Sqlite);
  }

  #[tokio::test]
  async fn test_init_db_runs_migrations() {
    use crate::db::migrations::Migrator;
    use sea_orm::EntityTrait;

    let config = DBConfig::default();
    // init_db applies all migrations; the user table must then be queryable.
    let conn = init_db::<Migrator>(&config, "sqlite::memory:").await;
    assert!(
      crate::db::entities::user::Entity::find()
        .all(&*conn)
        .await
        .is_ok()
    );
  }
}
