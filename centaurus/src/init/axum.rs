use std::net::SocketAddr;

use axum::{Router, serve};
use tokio::{net::TcpListener, signal};
use tower::ServiceBuilder;

pub async fn listener_setup(port: u16) -> TcpListener {
  let addr = SocketAddr::from(([0, 0, 0, 0], port));

  TcpListener::bind(addr)
    .await
    .expect("Failed to bind to address")
}

pub async fn run_app(listener: TcpListener, app: Router) {
  serve(listener, app)
    .with_graceful_shutdown(shutdown_signal())
    .await
    .expect("Failed to start server");
}

pub async fn run_app_connect_info(listener: TcpListener, app: Router) {
  serve(
    listener,
    app.into_make_service_with_connect_info::<SocketAddr>(),
  )
  .with_graceful_shutdown(shutdown_signal())
  .await
  .expect("Failed to start server");
}

pub async fn shutdown_signal() {
  let ctrl_c = async {
    signal::ctrl_c()
      .await
      .expect("failed to install Ctrl+C handler");
  };

  let terminate = async {
    signal::unix::signal(signal::unix::SignalKind::terminate())
      .expect("failed to install signal handler")
      .recv()
      .await;
  };

  tokio::select! {
      _ = ctrl_c => {},
      _ = terminate => {},
  }
}

#[allow(non_camel_case_types, async_fn_in_trait)]
pub trait add_base_layers {
  async fn add_base_layers(self, config: &crate::config::BaseConfig) -> Self;

  async fn add_base_layers_filtered<F: Fn(&str) -> bool + Clone + Send + Sync + 'static>(
    self,
    config: &crate::config::BaseConfig,
    filter: F,
  ) -> Self;
}

impl add_base_layers for axum::Router {
  async fn add_base_layers(self, config: &crate::config::BaseConfig) -> Self {
    self.add_base_layers_filtered(config, |_| true).await
  }

  async fn add_base_layers_filtered<F: Fn(&str) -> bool + Clone + Send + Sync + 'static>(
    self,
    config: &crate::config::BaseConfig,
    filter: F,
  ) -> Self {
    #[cfg(feature = "logging")]
    use super::logging::logging;

    #[allow(unused_mut)]
    let mut router = self;

    router = router.layer(
      ServiceBuilder::new().layer(super::cors::cors(config).expect("Failed to build CORS layer")),
    );

    #[cfg(feature = "logging")]
    {
      router = router.logging(filter);
    }

    router
  }
}
