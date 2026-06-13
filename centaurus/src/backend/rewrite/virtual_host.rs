use axum::{Router, extract::Request, response::Response};
use axum_extra::headers::{HeaderMapExt, Host};
use tower_service::Service;
use tracing::info;
use url::Url;

#[derive(Clone)]
pub struct HostRouter<S> {
  prefix: String,
  replace_path: String,
  inner: S,
}

impl<S, ResBody> Service<Request> for HostRouter<S>
where
  S: Service<Request, Response = Response<ResBody>>,
{
  type Error = S::Error;
  type Future = S::Future;
  type Response = S::Response;

  fn poll_ready(
    &mut self,
    cx: &mut std::task::Context<'_>,
  ) -> std::task::Poll<Result<(), Self::Error>> {
    self.inner.poll_ready(cx)
  }

  fn call(&mut self, mut req: Request) -> Self::Future {
    self.modify_req(&mut req);
    self.inner.call(req)
  }
}

impl HostRouter<Router> {
  /// replace_path must contain '{subdomain}' and '{path}' which will be replaced with the subdomain and the original path respectively.
  pub fn new(router: Router, url: Url, replace_path: String) -> Self {
    let Some(host) = url.host() else {
      panic!("Virtual host routing is enabled, but the site URL does not contain a host");
    };
    let url::Host::Domain(host) = host else {
      panic!("Virtual host routing is enabled, but the site URL does not contain a valid host");
    };
    let subdomain = subdomain_from_host(host).unwrap_or_default();

    info!("Virtual host routing enabled with subdomain prefix: {subdomain}");

    Self {
      prefix: subdomain,
      replace_path,
      inner: router.clone(),
    }
  }
}

impl<S> HostRouter<S> {
  fn modify_req(&self, req: &mut Request) -> Option<()> {
    let host = req.headers().typed_get::<Host>()?;
    let subdomain = subdomain_from_host(host.hostname())?;

    let suffix = if self.prefix.is_empty() {
      ""
    } else {
      &format!(".{}", self.prefix)
    };
    let subdomain_part = subdomain.strip_suffix(suffix)?;

    let path = req.uri().path();
    let new_path = self
      .replace_path
      .replace("{subdomain}", subdomain_part)
      .replace("{path}", path);

    let mut parts = req.uri().clone().into_parts();
    parts.path_and_query = Some(new_path.parse().ok()?);
    let new_uri = http::Uri::from_parts(parts).ok()?;
    *req.uri_mut() = new_uri;

    Some(())
  }
}

fn subdomain_from_host(host: &str) -> Option<String> {
  let domain = addr::parse_domain_name(host).ok()?;
  domain.prefix().map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
  use super::*;
  use axum::body::Body;

  #[test]
  fn test_subdomain_from_host() {
    assert_eq!(
      subdomain_from_host("tenant.example.com"),
      Some("tenant".to_string())
    );
    assert_eq!(
      subdomain_from_host("a.b.example.com"),
      Some("a.b".to_string())
    );
    // A bare registrable domain has no prefix/subdomain.
    assert_eq!(subdomain_from_host("example.com"), None);
  }

  fn request_with_host(host: &str, path: &str) -> Request {
    Request::builder()
      .uri(path)
      .header("host", host)
      .body(Body::empty())
      .unwrap()
  }

  #[test]
  fn test_modify_req_with_prefix() {
    // Site at app.example.com → prefix "app". Tenant requests arrive as
    // <tenant>.app.example.com and must be rewritten to /<tenant><path>.
    let url = Url::parse("https://app.example.com").unwrap();
    let router = HostRouter::new(Router::new(), url, "/{subdomain}{path}".into());

    let mut req = request_with_host("tenant.app.example.com", "/dashboard");
    assert!(router.modify_req(&mut req).is_some());
    assert_eq!(req.uri().path(), "/tenant/dashboard");
  }

  #[test]
  fn test_modify_req_without_prefix() {
    // Site at example.com → empty prefix. The full subdomain is the tenant.
    let url = Url::parse("https://example.com").unwrap();
    let router = HostRouter::new(Router::new(), url, "/{subdomain}{path}".into());

    let mut req = request_with_host("tenant.example.com", "/x");
    assert!(router.modify_req(&mut req).is_some());
    assert_eq!(req.uri().path(), "/tenant/x");
  }

  #[test]
  fn test_modify_req_no_subdomain_is_noop() {
    let url = Url::parse("https://app.example.com").unwrap();
    let router = HostRouter::new(Router::new(), url, "/{subdomain}{path}".into());

    // A request whose host lacks the configured suffix is left untouched.
    let mut req = request_with_host("example.com", "/unchanged");
    assert!(router.modify_req(&mut req).is_none());
    assert_eq!(req.uri().path(), "/unchanged");
  }

  #[test]
  #[should_panic]
  fn test_new_panics_without_domain_host() {
    // An IP-based site URL has no domain host and must be rejected.
    let url = Url::parse("https://127.0.0.1:8000").unwrap();
    let _ = HostRouter::new(Router::new(), url, "/{subdomain}{path}".into());
  }
}
