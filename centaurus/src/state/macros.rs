#[cfg(feature = "axum")]
#[macro_export]
macro_rules! router_extension {
  (async fn $name:ident($($arg:tt)*) -> Self { $($body:tt)* }) => {
    $crate::router_extension!($crate::axum::Router, async fn $name($($arg)*) -> Self { $($body)* });
  };
  ($self:ty, async fn $name:ident($($arg:tt)*) -> Self { $($body:tt)* }) => {
    #[allow(non_camel_case_types, async_fn_in_trait)]
    pub trait $name {
      async fn $name($($arg)*) -> Self;
    }

    impl $name for $self {
      async fn $name($($arg)*) -> Self {
        $($body)*
      }
    }
  };
}
