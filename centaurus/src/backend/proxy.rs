use axum::{
  Extension,
  body::Body,
  extract::{FromRequestParts, Request},
  response::{IntoResponse, Response},
  routing::get,
};
use http::StatusCode;
use hyper_util::{client::legacy::connect::HttpConnector, rt::TokioExecutor};
use tracing::instrument;

use crate::backend::BackendRouter;

type Client = hyper_util::client::legacy::Client<HttpConnector, Body>;

pub trait ProxyExt {
  fn proxy(self, from: String, to: String) -> Self;
}

impl ProxyExt for BackendRouter {
  /// from and to both must have a trailing slash, e.g. "/proxy/" or "/"
  fn proxy(self, from: String, to: String) -> Self {
    self
      .route(&format!("{}{{*p}}", from), get(handler))
      .route(&from, get(handler))
      .layer(Extension(ProxyState {
        client: hyper_util::client::legacy::Client::<(), ()>::builder(TokioExecutor::new())
          .build(HttpConnector::new()),
        proxy_url: to,
        rewrite: from,
      }))
  }
}

#[derive(FromRequestParts, Clone, Debug)]
#[cfg_attr(feature = "openapi", derive(aide::OperationIo))]
#[from_request(via(Extension))]
struct ProxyState {
  client: Client,
  proxy_url: String,
  rewrite: String,
}

#[instrument(level = "trace", skip(state, req))]
async fn handler(state: ProxyState, mut req: Request) -> Result<Response, StatusCode> {
  tracing::trace!("Forwarding request to frontend: {}", req.uri());
  let path = req.uri().path();
  let path = path.strip_prefix(&state.rewrite).unwrap_or(path);
  let query = req
    .uri()
    .query()
    .map(|q| format!("?{}", q))
    .unwrap_or_default();

  let uri = format!("{}{}{}", state.proxy_url, path, query);
  *req.uri_mut() = uri.parse().map_err(|_| StatusCode::BAD_REQUEST)?;

  Ok(
    state
      .client
      .request(req)
      .await
      .map_err(|_| StatusCode::BAD_GATEWAY)?
      .into_response(),
  )
}
