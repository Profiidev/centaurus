use serde::{Serialize, de::DeserializeOwned};

#[cfg(feature = "openapi")]
pub trait Settings: Serialize + DeserializeOwned + Default + schemars::JsonSchema {
  fn id() -> i32;
}

#[cfg(not(feature = "openapi"))]
pub trait Settings: Serialize + DeserializeOwned + Default {
  fn id() -> i32;
}
