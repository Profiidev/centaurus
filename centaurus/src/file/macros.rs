#[macro_export]
macro_rules! path {
  () => {
    std::path::PathBuf::new()
  };
  ($($segment:expr),+ $(,)?) => {
    {
      let mut path = std::path::PathBuf::new();
      $(
        path.push($segment);
      )+
      path
    }
  };
}

#[macro_export]
macro_rules! overwrite_with_env_config {
  ($config:ident, $env_config:ident, $($field:ident),*,,$($bool_field:ident),*) => {
    if let Some($env_config) = $env_config {
      $(
        if let Some($field) = &$env_config.$field {
          $config.$field = Some($field.clone());
        }
      )*

      $(
        if let Some($bool_field) = $env_config.$bool_field {
          $config.$bool_field = Some($bool_field);
        }
      )*
    }
  };
}

#[cfg(test)]
mod tests {
  #[test]
  fn test_path_macro() {
    let p = path!("a", "b", "c");
    assert_eq!(p, std::path::PathBuf::from("a/b/c"));

    let p = path!();
    assert_eq!(p, std::path::PathBuf::new());
  }
}
