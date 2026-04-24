use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, parse_macro_input};

use crate::centaurus_path;

pub fn update_message(input: TokenStream) -> TokenStream {
  let input = parse_macro_input!(input as DeriveInput);
  let name = &input.ident;

  let data = match input.data {
    Data::Enum(ref data) => data,
    _ => {
      return syn::Error::new_spanned(name, "UpdateMessage derive only supports enums")
        .to_compile_error()
        .into();
    }
  };

  let mut settings_variant = None;
  let mut group_variant = None;
  let mut user_variant = None;
  let mut permissions_variant = None;

  // Parse variants and attributes
  for variant in &data.variants {
    for attr in &variant.attrs {
      if attr.path().is_ident("update_message")
        && let Err(e) = attr.parse_nested_meta(|meta| {
          let v_name = &variant.ident;
          if meta.path.is_ident("settings") {
            settings_variant = Some(quote!(Self::#v_name));
          } else if meta.path.is_ident("group") {
            group_variant = Some(quote!(Self::#v_name { uuid }));
          } else if meta.path.is_ident("user") {
            user_variant = Some(quote!(Self::#v_name { uuid }));
          } else if meta.path.is_ident("user_permissions") {
            permissions_variant = Some(quote!(Self::#v_name));
          }
          Ok(())
        })
      {
        return e.to_compile_error().into();
      }
    }
  }

  // Ensure all required methods found a mapping
  let Some(settings) = settings_variant else {
    return syn::Error::new_spanned(name, "Missing #[update_message(settings)] variant")
      .to_compile_error()
      .into();
  };
  let Some(group) = group_variant else {
    return syn::Error::new_spanned(name, "Missing #[update_message(group)] variant")
      .to_compile_error()
      .into();
  };
  let Some(user) = user_variant else {
    return syn::Error::new_spanned(name, "Missing #[update_message(user)] variant")
      .to_compile_error()
      .into();
  };
  let Some(permissions) = permissions_variant else {
    return syn::Error::new_spanned(name, "Missing #[update_message(user_permissions)] variant")
      .to_compile_error()
      .into();
  };

  let path = centaurus_path();
  quote! {
      impl #path::backend::endpoints::websocket::state::UpdateMessage for #name {
          fn settings() -> Self { #settings }
          fn group(uuid: uuid::Uuid) -> Self { #group }
          fn user(uuid: uuid::Uuid) -> Self { #user }
          fn user_permissions() -> Self { #permissions }
      }
  }
  .into()
}
