use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DBConfig {
  pub database_max_connections: u32,
  pub database_min_connections: u32,
  pub database_connect_timeout: u64,
  pub database_logging: bool,
}
