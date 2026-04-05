#[macro_export]
macro_rules! version_header {
  ($router:ident) => {
    const API_VERSION: $crate::http::HeaderValue =
      $crate::http::HeaderValue::from_static(env!("CARGO_PKG_VERSION"));

    async fn version_middleware(
      request: $crate::axum::extract::Request,
      next: $crate::axum::middleware::Next,
    ) -> $crate::axum::response::Response {
      let mut response = next.run(request).await;
      response
        .headers_mut()
        .insert($crate::VERSION_HEADER_NAME, API_VERSION);
      response
    }

    $router = $router.layer($crate::axum::middleware::from_fn(version_middleware));
  };
}
