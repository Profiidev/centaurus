use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Ident, parse_macro_input};

use crate::centaurus_path;

pub fn config(input: TokenStream) -> TokenStream {
  let input = parse_macro_input!(input as DeriveInput);
  let name = input.ident.clone();

  let files = match &input.data {
    Data::Struct(data) => match &data.fields {
      Fields::Named(fields) => &fields.named,
      _ => {
        return syn::Error::new_spanned(
          &input,
          "Config can only be derived for structs with named fields",
        )
        .to_compile_error()
        .into();
      }
    },
    _ => {
      return syn::Error::new_spanned(
        &input,
        "Config can only be derived for structs with named fields",
      )
      .to_compile_error()
      .into();
    }
  };

  let mut base_field: Option<Ident> = None;
  let mut metrics_field: Option<Ident> = None;
  let mut site_field: Option<Ident> = None;

  for field in files {
    for attr in &field.attrs {
      if attr.path().is_ident("base") {
        if base_field.is_some() {
          return syn::Error::new_spanned(
            &field.ident,
            "Multiple fields with #[base] attribute found",
          )
          .to_compile_error()
          .into();
        }
        base_field = field.ident.clone();
      } else if attr.path().is_ident("metrics") {
        if metrics_field.is_some() {
          return syn::Error::new_spanned(
            &field.ident,
            "Multiple fields with #[metrics] attribute found",
          )
          .to_compile_error()
          .into();
        }
        metrics_field = field.ident.clone();
      } else if attr.path().is_ident("site") {
        if site_field.is_some() {
          return syn::Error::new_spanned(
            &field.ident,
            "Multiple fields with #[site] attribute found",
          )
          .to_compile_error()
          .into();
        }
        site_field = field.ident.clone();
      }
    }
  }

  let Some(base_field) = base_field else {
    return syn::Error::new_spanned(&input, "No field with #[base] attribute found")
      .to_compile_error()
      .into();
  };

  let path = centaurus_path();
  let metrics_impl = if cfg!(feature = "metrics") {
    let Some(metrics_field) = metrics_field else {
      return syn::Error::new_spanned(&input, "No field with #[metrics] attribute found")
        .to_compile_error()
        .into();
    };
    quote! {
      fn metrics(&self) -> &#path::backend::config::MetricsConfig {
        &self.#metrics_field
      }
    }
  } else {
    quote! {}
  };

  let site_impl = if cfg!(feature = "site") {
    let Some(site_field) = site_field else {
      return syn::Error::new_spanned(&input, "No field with #[site] attribute found")
        .to_compile_error()
        .into();
    };
    quote! {
      fn site(&self) -> &#path::backend::config::SiteConfig {
        &self.#site_field
      }
    }
  } else {
    quote! {}
  };

  quote! {
    impl #path::backend::config::Config for #name {
      fn base(&self) -> &#path::backend::config::BaseConfig {
        &self.#base_field
      }

      #metrics_impl

      #site_impl
    }
  }
  .into()
}
