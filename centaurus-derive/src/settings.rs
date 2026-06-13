use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, Expr, parse2};

use crate::centaurus_path;

pub fn settings(input: TokenStream) -> TokenStream {
  let input = match parse2::<DeriveInput>(input) {
    Ok(input) => input,
    Err(err) => return err.to_compile_error(),
  };
  let name = input.ident.clone();

  let mut id_value = None;

  for attr in &input.attrs {
    if attr.path().is_ident("settings")
      && let Err(e) = attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("id") {
          let value: Expr = meta.value()?.parse()?;
          id_value = Some(quote! { #value });
          Ok(())
        } else {
          Err(meta.error("Unknown attribute in #[settings]: expected 'id'"))
        }
      })
    {
      return e.to_compile_error();
    }
  }

  let path = centaurus_path();
  quote! {
    impl #path::db::settings::Settings for #name {
      fn id() -> i32 {
        #id_value
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_settings_derive() {
    let input = quote! {
      #[settings(id = 1)]
      struct MySettings {}
    };
    let output = settings(input);
    let output_str = output.to_string();
    assert!(output_str.contains("impl centaurus :: db :: settings :: Settings for MySettings"));
    assert!(output_str.contains("fn id () -> i32 { 1 }"));
  }
}
