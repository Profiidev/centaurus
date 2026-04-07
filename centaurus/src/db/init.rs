use std::{ops::Deref, time::Duration};

use sea_orm::{
  ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement,
};
use sea_orm_migration::MigratorTrait;
use tracing::instrument;

use crate::db::config::DBConfig;

#[derive(Clone)]
#[cfg_attr(feature = "openapi", derive(aide::OperationIo))]
#[cfg_attr(feature = "axum", derive(axum::extract::FromRequestParts))]
#[cfg_attr(feature = "axum", from_request(via(axum::Extension)))]
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
