use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, Expr, parse_macro_input};

use crate::centaurus_path;

pub fn settings(input: TokenStream) -> TokenStream {
  let input = parse_macro_input!(input as DeriveInput);
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
      return e.to_compile_error().into();
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
  .into()
}
