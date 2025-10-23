use std::{ops::Deref, time::Duration};

use centaurus_derive::FromReqExtension;
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;
use tracing::instrument;

use crate as centaurus;
use crate::db::config::DBConfig;

#[derive(FromReqExtension, Clone)]
pub struct Connection(DatabaseConnection);

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

  Connection(conn)
}
