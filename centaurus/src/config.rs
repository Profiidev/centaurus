use clap::Args;
#[cfg(feature = "tracing")]
use tracing::Level;

#[derive(Args, Clone)]
pub struct BaseConfig {
  //base
  #[clap(long, env, default_value = "8000")]
  pub port: u16,

  #[cfg(feature = "tracing")]
  #[clap(long, env, default_value = "info")]
  pub log_level: Level,

  #[clap(long, env, default_value = "")]
  pub allowed_origins: String,
}
