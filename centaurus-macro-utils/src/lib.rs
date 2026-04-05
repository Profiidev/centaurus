extern crate proc_macro;

use std::{env, path::PathBuf};

use proc_macro::TokenStream;
use toml_edit::{DocumentMut, Item};

const CENTAURUS: &str = "centaurus";

pub struct Manifest {
  doc: DocumentMut,
  crate_name: String,
}

impl Default for Manifest {
  fn default() -> Self {
    Self {
      doc: env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .map(|mut path| {
          path.push("Cargo.toml");
          if !path.exists() {
            panic!("No Cargo.toml found. Expected: {}", path.display());
          }
          let manifest = std::fs::read_to_string(path.clone())
            .unwrap_or_else(|_| panic!("Unable to read Cargo.toml: {}", path.display()));
          manifest
            .parse::<DocumentMut>()
            .unwrap_or_else(|_| panic!("Failed to parse Cargo.toml: {}", path.display()))
        })
        .expect("CARGO_MANIFEST_DIR not defined."),
      crate_name: CENTAURUS.to_string(),
    }
  }
}

impl Manifest {
  pub fn get_path(&self, name: &str) -> syn::Path {
    self.try_get_path(name).unwrap_or_else(|| parse_str(name))
  }

  pub fn try_get_path(&self, name: &str) -> Option<syn::Path> {
    if let Some(package) = self.doc.get("package")
      && let Some(p_name) = package.get("name").and_then(|n| n.as_str())
      && name == p_name
    {
      return Some(parse_str("crate"));
    }

    fn dep_package(dep: &Item) -> Option<&str> {
      if dep.as_str().is_some() {
        None
      } else {
        dep.get("package").map(|name| name.as_str().unwrap())
      }
    }

    let find = |d: &Item| {
      let dep = if let Some(dep) = d.get(name) {
        return Some(parse_str(dep_package(dep).unwrap_or(name)));
      } else if let Some(dep) = d.get(&self.crate_name) {
        dep_package(dep).unwrap_or(&self.crate_name)
      } else {
        return None;
      };

      let mut path = parse_str::<syn::Path>(dep);
      if let Some(module) = name.strip_prefix(&format!("{}_", self.crate_name)) {
        path.segments.push(parse_str(module));
      } else if let Some(module) = name.strip_prefix(&format!("{}-", self.crate_name)) {
        path.segments.push(parse_str(module));
      }
      Some(path)
    };

    let dependencies = self.doc.get("dependencies");
    let dev_dependencies = self.doc.get("dev-dependencies");

    dependencies
      .and_then(find)
      .or_else(|| dev_dependencies.and_then(find))
  }
}

fn try_parse_str<T: syn::parse::Parse>(path: &str) -> Option<T> {
  syn::parse(path.parse::<TokenStream>().ok()?).ok()
}

fn parse_str<T: syn::parse::Parse>(path: &str) -> T {
  try_parse_str(path).unwrap()
}
