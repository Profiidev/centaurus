use std::{ops::Deref, time::Instant};

use crate as centaurus;
use ::metrics::{
  Unit, counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram,
};
use axum::{
  Extension, RequestExt,
  extract::Request,
  middleware::{Next, from_fn},
  response::Response,
  routing::get,
};
use centaurus_derive::FromReqExtension;
use http::HeaderMap;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

use crate::router_extension;

#[derive(FromReqExtension, Clone)]
pub struct MetricsHandle {
  prometheus_handle: PrometheusHandle,
}

impl Deref for MetricsHandle {
  type Target = PrometheusHandle;

  fn deref(&self) -> &Self::Target {
    &self.prometheus_handle
  }
}

#[cfg_attr(feature = "logging", tracing::instrument)]
pub fn init_metrics(service_name: String) -> Extension<MetricsHandle> {
  let builder = PrometheusBuilder::new().add_global_label("service_name", service_name);
  let handle = builder
    .install_recorder()
    .expect("failed to install Prometheus recorder");

  Extension(MetricsHandle {
    prometheus_handle: handle,
  })
}

router_extension!(
  async fn metrics_route(self) -> Self {
    self.route(
      "/metrics",
      get(async |handle: MetricsHandle| handle.render()),
    )
  }
);

#[derive(Clone, FromReqExtension)]
struct MetricsPrefix(String);

router_extension!(
  async fn metrics(self, metrics_prefix: String) -> Self {
    describe_metrics(&metrics_prefix);
    self
      .layer(from_fn(request_metrics))
      .layer(Extension(MetricsPrefix(metrics_prefix)))
  }
);

fn describe_metrics(prefix: &str) {
  describe_counter!(
    format!("{}_http_requests_total", prefix),
    Unit::Count,
    "Total number of HTTP requests received"
  );
  describe_counter!(
    format!("{}_http_requests_successfull", prefix),
    Unit::Count,
    "Total number of successful HTTP requests"
  );
  describe_counter!(
    format!("{}_http_requests_failed", prefix),
    Unit::Count,
    "Total number of failed HTTP requests"
  );
  describe_histogram!(
    format!("{}_http_request_body_size", prefix),
    Unit::Bytes,
    "Size of HTTP requests in bytes"
  );
  describe_histogram!(
    format!("{}_http_response_body_size", prefix),
    Unit::Bytes,
    "Size of HTTP responses in bytes"
  );
  describe_histogram!(
    format!("{}_http_request_duration", prefix),
    Unit::Milliseconds,
    "Duration of HTTP requests in milliseconds"
  );
  describe_gauge!(
    format!("{}_http_active_requests", prefix),
    Unit::Count,
    "Number of active HTTP requests"
  );
}

async fn request_metrics(mut req: Request, next: Next) -> Response {
  let start = Instant::now();
  let scheme = scheme(&req);
  let method = req.method().to_string();
  let path = req.uri().path().to_string();

  let Ok(MetricsPrefix(prefix)) = req.extract_parts::<MetricsPrefix>().await;

  let labels = [
    ("http.request.method", method),
    ("url.path", path),
    ("url.scheme", scheme),
  ];

  gauge!(format!("{}_http_active_requests", prefix), &labels).increment(1);
  counter!(format!("{}_http_requests_total", prefix), &labels).increment(1);

  let size = content_size(req.headers());
  histogram!(format!("{}_http_request_size", prefix), &labels).record(size as f64);

  let response = next.run(req).await;

  if response.status().is_success() {
    counter!(format!("{}_http_requests_successfull", prefix), &labels).increment(1);
  } else {
    counter!(format!("{}_http_requests_failed", prefix), &labels).increment(1);
  }

  let size = content_size(response.headers());
  histogram!(format!("{}_http_response_size", prefix), &labels).record(size as f64);

  let duration = start.elapsed().as_millis() as f64;
  histogram!(format!("{}_http_request_duration", prefix), &labels).record(duration);

  gauge!(format!("{}_http_active_requests", prefix), &labels).decrement(1);

  response
}

fn content_size(headers: &HeaderMap) -> f64 {
  headers
    .get(http::header::CONTENT_LENGTH)
    .and_then(|value| value.to_str().ok())
    .and_then(|s| s.parse::<f64>().ok())
    .unwrap_or(0.0)
}

fn scheme(req: &Request) -> String {
  if let Some(scheme) = req.headers().get("X-Forwarded-Prot") {
    scheme.to_str().unwrap_or("http").to_string()
  } else if let Some(scheme) = req.headers().get("X-Forwarded-Protocol") {
    scheme.to_str().unwrap_or("http").to_string()
  } else if let Some(ssl) = req.headers().get("X-Forwarded-Ssl") {
    if ssl.to_str().unwrap_or("off") == "on" {
      "https".to_string()
    } else {
      "http".to_string()
    }
  } else if let Some(scheme) = req.headers().get("X-Url-Scheme") {
    scheme.to_str().unwrap_or("http").to_string()
  } else if let Some(scheme) = req.uri().scheme() {
    scheme.as_str().to_string()
  } else {
    "http".to_string()
  }
}
