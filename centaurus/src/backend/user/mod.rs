use crate::backend::{middleware::rate_limiter::RateLimiter, websocket::state::UpdateMessage};
use aide::axum::ApiRouter;

mod account;
mod info;
mod management;
mod template;

pub fn router<T: UpdateMessage>(rate_limiter: &mut RateLimiter) -> ApiRouter {
  ApiRouter::new()
    .nest("/account", account::router::<T>(rate_limiter))
    .nest("/info", info::router())
    .nest("/management", management::router::<T>())
}
