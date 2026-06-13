use serde::{Deserialize, Serialize};
use tracing::warn;

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
      database_max_connections: 20,
      database_min_connections: 1,
      database_connect_timeout: 5,
      database_logging: false,
    }
  }
}

impl DBConfig {
  pub fn validate_sqlite(&mut self) {
    if self.database_max_connections > 1 {
      self.database_max_connections = 1;
      if self.database_max_connections != DBConfig::default().database_max_connections {
        warn!(
          "SQLite does not work properly with multiple connections. Setting DATABASE_MAX_CONNECTIONS to 1."
        );
      }
    }

    if self.database_min_connections > 1 {
      self.database_min_connections = 1;
      if self.database_min_connections != DBConfig::default().database_min_connections {
        warn!(
          "SQLite does not work properly with multiple connections. Setting DATABASE_MIN_CONNECTIONS to 1."
        );
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_db_config_default() {
    let config = DBConfig::default();
    assert_eq!(config.database_max_connections, 20);
  }

  #[test]
  fn test_validate_sqlite() {
    let mut config = DBConfig {
      database_max_connections: 5,
      database_min_connections: 5,
      ..Default::default()
    };
    config.validate_sqlite();
    assert_eq!(config.database_max_connections, 1);
    assert_eq!(config.database_min_connections, 1);
  }
}
