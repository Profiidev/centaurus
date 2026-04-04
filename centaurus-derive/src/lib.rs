use centaurus_macro_utils::Manifest;
use proc_macro::TokenStream;

use crate::{config::config, settings::settings};

mod config;
mod settings;

pub(crate) fn centaurus_path() -> syn::Path {
  Manifest::default().get_path("centaurus")
}

#[proc_macro_derive(Config, attributes(base, metrics))]
pub fn derive_config(input: TokenStream) -> TokenStream {
  config(input)
}

#[proc_macro_derive(Settings, attributes(settings))]
pub fn derive_settings(input: TokenStream) -> TokenStream {
  settings(input)
}
