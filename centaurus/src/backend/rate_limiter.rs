use std::{
  net::IpAddr,
  sync::Arc,
  thread::{sleep, spawn},
  time::Duration,
};

use dashmap::DashMap;
use governor::{clock::QuantaClock, middleware::StateInformationMiddleware, state::InMemoryState};
use tower_governor::{
  governor::{GovernorConfig, GovernorConfigBuilder},
  key_extractor::SmartIpKeyExtractor,
};

pub type Governor = GovernorConfig<SmartIpKeyExtractor, StateInformationMiddleware>;
type Limiter = Arc<
  governor::RateLimiter<
    IpAddr,
    DashMap<IpAddr, InMemoryState>,
    QuantaClock,
    StateInformationMiddleware,
  >,
>;

#[derive(Default)]
pub struct RateLimiter {
  cleaner: Vec<Limiter>,
}

impl RateLimiter {
  pub fn create_limiter(&mut self) -> Governor {
    let conf = GovernorConfigBuilder::default()
      .key_extractor(SmartIpKeyExtractor)
      .per_second(10)
      .burst_size(20)
      .use_headers()
      .finish()
      .unwrap();

    self.cleaner.push(conf.limiter().clone());

    conf
  }

  pub fn init(self) {
    spawn(move || {
      loop {
        sleep(Duration::from_secs(600));
        tracing::debug!("Cleaning rate limiter state");
        for limiter in &self.cleaner {
          limiter.retain_recent();
        }
      }
    });
  }
}
