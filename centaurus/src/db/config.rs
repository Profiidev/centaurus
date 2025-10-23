use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DBConfig {
  pub database_max_connections: u32,
  pub database_min_connections: u32,
  pub database_connect_timeout: u64,
  pub database_logging: bool,
}

impl Default for DBConfig {
  fn default() -> Self {
    Self {
      database_max_connections: 1024,
      database_min_connections: 1,
      database_connect_timeout: 5,
      database_logging: false,
    }
  }
}
