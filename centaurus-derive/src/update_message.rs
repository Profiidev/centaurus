use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, parse2};

use crate::centaurus_path;

pub fn update_message(input: TokenStream) -> TokenStream {
  let input = match parse2::<DeriveInput>(input) {
    Ok(input) => input,
    Err(err) => return err.to_compile_error(),
  };
  let name = &input.ident;

  let data = match input.data {
    Data::Enum(ref data) => data,
    _ => {
      return syn::Error::new_spanned(name, "UpdateMessage derive only supports enums")
        .to_compile_error();
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
        return e.to_compile_error();
      }
    }
  }

  // Ensure all required methods found a mapping
  let Some(settings) = settings_variant else {
    return syn::Error::new_spanned(name, "Missing #[update_message(settings)] variant")
      .to_compile_error();
  };
  let Some(group) = group_variant else {
    return syn::Error::new_spanned(name, "Missing #[update_message(group)] variant")
      .to_compile_error();
  };
  let Some(user) = user_variant else {
    return syn::Error::new_spanned(name, "Missing #[update_message(user)] variant")
      .to_compile_error();
  };
  let Some(permissions) = permissions_variant else {
    return syn::Error::new_spanned(name, "Missing #[update_message(user_permissions)] variant")
      .to_compile_error();
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
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_update_message_derive() {
    let input = quote! {
      enum MyUpdate {
        #[update_message(settings)]
        Settings,
        #[update_message(group)]
        Group { uuid: uuid::Uuid },
        #[update_message(user)]
        User { uuid: uuid::Uuid },
        #[update_message(user_permissions)]
        Permissions,
      }
    };
    let output = update_message(input);
    let output_str = output.to_string();
    assert!(output_str.contains(
      "impl centaurus :: backend :: endpoints :: websocket :: state :: UpdateMessage for MyUpdate"
    ));
    assert!(output_str.contains("fn settings () -> Self { Self :: Settings }"));
  }

  #[test]
  fn test_update_message_rejects_non_enum() {
    let input = quote! {
      struct NotAnEnum {
        field: u32,
      }
    };
    assert!(
      update_message(input)
        .to_string()
        .contains("UpdateMessage derive only supports enums")
    );
  }

  #[test]
  fn test_update_message_missing_settings_variant() {
    let input = quote! {
      enum MyUpdate {
        #[update_message(group)]
        Group { uuid: uuid::Uuid },
        #[update_message(user)]
        User { uuid: uuid::Uuid },
        #[update_message(user_permissions)]
        Permissions,
      }
    };
    assert!(
      update_message(input)
        .to_string()
        .contains("Missing #[update_message(settings)] variant")
    );
  }

  #[test]
  fn test_update_message_missing_user_variant() {
    let input = quote! {
      enum MyUpdate {
        #[update_message(settings)]
        Settings,
        #[update_message(group)]
        Group { uuid: uuid::Uuid },
        #[update_message(user_permissions)]
        Permissions,
      }
    };
    assert!(
      update_message(input)
        .to_string()
        .contains("Missing #[update_message(user)] variant")
    );
  }

  #[test]
  fn test_update_message_invalid_tokens() {
    assert!(
      update_message(quote! { not valid })
        .to_string()
        .contains("error")
    );
  }
}
