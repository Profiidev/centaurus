use clap::Args;
#[cfg(feature = "logging")]
use tracing::Level;

#[derive(Args, Clone)]
pub struct BaseConfig {
  //base
  #[clap(long, env, default_value = "8000")]
  pub port: u16,

  #[cfg(feature = "logging")]
  #[clap(long, env, default_value = "info")]
  pub log_level: Level,

  #[clap(long, env, default_value = "")]
  pub allowed_origins: String,
}
