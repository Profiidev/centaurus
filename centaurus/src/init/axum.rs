use std::net::SocketAddr;

use axum::{Router, serve};
use tokio::{net::TcpListener, signal};
#[cfg(feature = "config")]
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

async fn shutdown_signal() {
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

#[cfg(feature = "config")]
crate::router_extension!(
  async fn add_base_layers(self, config: &crate::config::BaseConfig) -> Self {
    #[cfg(feature = "logging")]
    use super::logging::logging;

    #[allow(unused_mut)]
    let mut router = self;

    #[cfg(feature = "error")]
    {
      router = router.layer(
        ServiceBuilder::new().layer(super::cors::cors(config).expect("Failed to build CORS layer")),
      );
    }
    #[cfg(not(feature = "error"))]
    {
      router = router.layer(ServiceBuilder::new().layer(super::cors::cors(config)));
    }

    #[cfg(feature = "logging")]
    {
      router = router.logging().await;
    }

    router
  }
);
