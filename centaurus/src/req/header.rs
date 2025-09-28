#[macro_export]
macro_rules! typed_header {
  ($name:ident, $const:ident, $name_str:literal, $inner:ident, |$s:ident| $decode:expr, |$v:ident| $encode:expr) => {
    pub struct $name(pub $inner);
    pub static $const: centaurus::http::HeaderName =
      centaurus::http::HeaderName::from_static($name_str);

    impl std::ops::Deref for $name {
      type Target = $inner;

      fn deref(&self) -> &Self::Target {
        &self.0
      }
    }

    impl centaurus::axum_extra::headers::Header for $name {
      fn name() -> &'static centaurus::http::HeaderName {
        &$const
      }

      fn decode<'i, I>(values: &mut I) -> Result<Self, centaurus::axum_extra::headers::Error>
      where
        Self: Sized,
        I: Iterator<Item = &'i centaurus::http::HeaderValue>,
      {
        values
          .next()
          .and_then(|v| v.to_str().ok())
          .and_then(|$s| $decode)
          .map(|v| $name(v))
          .ok_or(centaurus::axum_extra::headers::Error::invalid())
      }

      fn encode<E: Extend<centaurus::http::HeaderValue>>(&self, values: &mut E) {
        let $v = &self.0;
        let value = $encode;
        let bytes = value.as_bytes();
        let val = centaurus::http::HeaderValue::from_bytes(bytes).expect("valid header value");
        values.extend(std::iter::once(val));
      }
    }
  };
}
