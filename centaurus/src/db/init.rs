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
  let mut options = ConnectOptions::new(connection_url);
  options
    .max_connections(config.database_max_connections)
    .min_connections(config.database_min_connections)
    .connect_timeout(Duration::from_secs(config.database_connect_timeout))
    .sqlx_logging(config.database_logging);

  let conn = Database::connect(options)
    .await
    .expect("Failed to connect to database");

  #[cfg(feature = "fix-migrations")]
  migrate_to_centaurus_migrations(&conn)
    .await
    .expect("Failed to migrate to centaurus migrations");

  M::up(&conn, None)
    .await
    .expect("Failed to run database migrations");

  if conn.get_database_backend() == DatabaseBackend::Sqlite {
    conn
      .execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        "PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 60000;".to_string(),
      ))
      .await
      .expect("Failed to set SQLite pragmas");
  }

  Connection(conn)
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
    tracing::warn!("No 'seaql_migrations' table found, skipping migration table rename");
  }

  let stmt = Statement::from_string(
    backend,
    "
    UPDATE seaql_migrations
    SET version = CASE
        WHEN version = 'm20250301_215149_create_key_table' THEN 'key'
        WHEN version = 'm20260123_144736_invalid_jwt'      THEN 'invalid_jwt'
        WHEN version = 'm20260123_144752_user'             THEN 'user'
        WHEN version = 'm20260126_155842_group'            THEN 'groups'
        WHEN version = 'm20260126_160754_setup'            THEN 'setup'
        ELSE version
    END
    WHERE version LIKE 'm20%';
  ",
  );
  conn.execute(stmt).await?;

  Ok(())
}
