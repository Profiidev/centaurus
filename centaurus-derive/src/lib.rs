use centaurus_macro_utils::Manifest;
use proc_macro::TokenStream;

use crate::{config::config, settings::settings, update_message::update_message};

mod config;
mod settings;
mod update_message;

pub(crate) fn centaurus_path() -> syn::Path {
  Manifest::default().get_path("centaurus")
}

#[proc_macro_derive(Config, attributes(base, metrics, site))]
pub fn derive_config(input: TokenStream) -> TokenStream {
  config(input)
}

#[proc_macro_derive(Settings, attributes(settings))]
pub fn derive_settings(input: TokenStream) -> TokenStream {
  settings(input)
}

#[proc_macro_derive(UpdateMessage, attributes(update_message))]
pub fn derive_update_message(input: TokenStream) -> TokenStream {
  update_message(input)
}
