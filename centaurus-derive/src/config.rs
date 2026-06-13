use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Ident, parse2};

use crate::centaurus_path;

pub fn config(input: TokenStream) -> TokenStream {
  let input = match parse2::<DeriveInput>(input) {
    Ok(input) => input,
    Err(err) => return err.to_compile_error(),
  };
  let name = input.ident.clone();

  let files = match &input.data {
    Data::Struct(data) => match &data.fields {
      Fields::Named(fields) => &fields.named,
      _ => {
        return syn::Error::new_spanned(
          &input,
          "Config can only be derived for structs with named fields",
        )
        .to_compile_error();
      }
    },
    _ => {
      return syn::Error::new_spanned(
        &input,
        "Config can only be derived for structs with named fields",
      )
      .to_compile_error();
    }
  };

  let mut base_field: Option<Ident> = None;
  let mut metrics_field: Option<Ident> = None;
  let mut site_field: Option<Ident> = None;
  let mut auth_field: Option<Ident> = None;
  let mut oidc_field: Option<Ident> = None;
  let mut mail_field: Option<Ident> = None;

  for field in files {
    for attr in &field.attrs {
      if attr.path().is_ident("base") {
        if base_field.is_some() {
          return syn::Error::new_spanned(
            &field.ident,
            "Multiple fields with #[base] attribute found",
          )
          .to_compile_error();
        }
        base_field = field.ident.clone();
      } else if attr.path().is_ident("metrics") {
        if metrics_field.is_some() {
          return syn::Error::new_spanned(
            &field.ident,
            "Multiple fields with #[metrics] attribute found",
          )
          .to_compile_error();
        }
        metrics_field = field.ident.clone();
      } else if attr.path().is_ident("site") {
        if site_field.is_some() {
          return syn::Error::new_spanned(
            &field.ident,
            "Multiple fields with #[site] attribute found",
          )
          .to_compile_error();
        }
        site_field = field.ident.clone();
      } else if attr.path().is_ident("oidc") {
        if oidc_field.is_some() {
          return syn::Error::new_spanned(
            &field.ident,
            "Multiple fields with #[oidc] attribute found",
          )
          .to_compile_error();
        }
        oidc_field = field.ident.clone();
      } else if attr.path().is_ident("mail") {
        if mail_field.is_some() {
          return syn::Error::new_spanned(
            &field.ident,
            "Multiple fields with #[mail] attribute found",
          )
          .to_compile_error();
        }
        mail_field = field.ident.clone();
      } else if attr.path().is_ident("auth") {
        if auth_field.is_some() {
          return syn::Error::new_spanned(
            &field.ident,
            "Multiple fields with #[auth] attribute found",
          )
          .to_compile_error();
        }
        auth_field = field.ident.clone();
      }
    }
  }

  let Some(base_field) = base_field else {
    return syn::Error::new_spanned(&input, "No field with #[base] attribute found")
      .to_compile_error();
  };

  let path = centaurus_path();
  let metrics_impl = if cfg!(feature = "metrics") {
    let Some(metrics_field) = metrics_field else {
      return syn::Error::new_spanned(&input, "No field with #[metrics] attribute found")
        .to_compile_error();
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
        .to_compile_error();
    };
    quote! {
      fn site(&self) -> &#path::backend::config::SiteConfig {
        &self.#site_field
      }
    }
  } else {
    quote! {}
  };

  let auth_impl = if cfg!(feature = "auth") {
    let Some(auth_field) = auth_field else {
      return syn::Error::new_spanned(&input, "No field with #[auth] attribute found")
        .to_compile_error();
    };
    quote! {
      fn auth(&self) -> &#path::backend::auth::settings::AuthConfig {
        &self.#auth_field
      }
    }
  } else {
    quote! {}
  };

  let oidc_impl = if let Some(oidc_field) = oidc_field {
    quote! {
      fn oidc(&self) -> Option<&#path::backend::auth::settings::UserSettings> {
        Some(&self.#oidc_field)
      }
    }
  } else {
    quote! {}
  };

  let mail_impl = if let Some(mail_field) = mail_field {
    quote! {
      fn mail(&self) -> Option<&#path::mail::MailSettings> {
        Some(&self.#mail_field)
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

      #auth_impl

      #oidc_impl

      #mail_impl
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_config_derive() {
    let input = quote! {
      struct MyConfig {
        #[base]
        base: BaseConfig,
        #[metrics]
        metrics: MetricsConfig,
        #[site]
        site: SiteConfig,
        #[auth]
        auth: AuthConfig,
      }
    };
    let output = config(input);
    let output_str = output.to_string();
    // When features are enabled, it should succeed if all fields are present
    if !output_str.contains("compile_error") {
      assert!(output_str.contains("impl centaurus :: backend :: config :: Config for MyConfig"));
      assert!(output_str.contains("fn base (& self) -> & centaurus :: backend :: config :: BaseConfig"));
    }
  }

  #[test]
  fn test_config_derive_no_base() {
    let input = quote! {
      struct MyConfig {
        field: String,
      }
    };
    let output = config(input);
    let output_str = output.to_string();
    assert!(output_str.contains("No field with #[base] attribute found"));
  }

  #[test]
  fn test_config_derive_full_with_oidc_and_mail() {
    let input = quote! {
      struct MyConfig {
        #[base]
        base: BaseConfig,
        #[metrics]
        metrics: MetricsConfig,
        #[site]
        site: SiteConfig,
        #[auth]
        auth: AuthConfig,
        #[oidc]
        oidc: UserSettings,
        #[mail]
        mail: MailSettings,
      }
    };
    let output = config(input).to_string();
    assert!(!output.contains("compile_error"));
    // The optional oidc/mail accessors are generated when their fields exist.
    assert!(output.contains("fn oidc"));
    assert!(output.contains("fn mail"));
  }

  #[test]
  fn test_config_derive_duplicate_base_errors() {
    let input = quote! {
      struct MyConfig {
        #[base]
        a: BaseConfig,
        #[base]
        b: BaseConfig,
      }
    };
    assert!(
      config(input)
        .to_string()
        .contains("Multiple fields with #[base] attribute found")
    );
  }

  #[test]
  fn test_config_derive_duplicate_oidc_errors() {
    let input = quote! {
      struct MyConfig {
        #[base]
        base: BaseConfig,
        #[oidc]
        a: UserSettings,
        #[oidc]
        b: UserSettings,
      }
    };
    assert!(
      config(input)
        .to_string()
        .contains("Multiple fields with #[oidc] attribute found")
    );
  }

  #[test]
  fn test_config_derive_duplicate_mail_errors() {
    let input = quote! {
      struct MyConfig {
        #[base]
        base: BaseConfig,
        #[mail]
        a: MailSettings,
        #[mail]
        b: MailSettings,
      }
    };
    assert!(
      config(input)
        .to_string()
        .contains("Multiple fields with #[mail] attribute found")
    );
  }

  #[test]
  fn test_config_derive_rejects_enum() {
    let input = quote! {
      enum NotAStruct {
        A,
      }
    };
    assert!(
      config(input)
        .to_string()
        .contains("Config can only be derived for structs with named fields")
    );
  }

  #[test]
  fn test_config_derive_rejects_tuple_struct() {
    let input = quote! {
      struct Tuple(u32, u32);
    };
    assert!(
      config(input)
        .to_string()
        .contains("Config can only be derived for structs with named fields")
    );
  }

  #[test]
  fn test_config_derive_invalid_tokens() {
    // A parse failure is surfaced as a compile error rather than panicking.
    let input = quote! { this is not valid rust };
    assert!(config(input).to_string().contains("error"));
  }

  // Feature-gated "missing field" branches. With the corresponding feature
  // enabled, omitting the field must produce a targeted error.
  #[cfg(feature = "metrics")]
  #[test]
  fn test_config_derive_missing_metrics_field() {
    let input = quote! {
      struct MyConfig {
        #[base]
        base: BaseConfig,
      }
    };
    assert!(
      config(input)
        .to_string()
        .contains("No field with #[metrics] attribute found")
    );
  }

  #[cfg(feature = "site")]
  #[test]
  fn test_config_derive_missing_site_field() {
    // Provide metrics (if required) but omit site.
    let input = quote! {
      struct MyConfig {
        #[base]
        base: BaseConfig,
        #[metrics]
        metrics: MetricsConfig,
      }
    };
    assert!(
      config(input)
        .to_string()
        .contains("No field with #[site] attribute found")
    );
  }

  #[cfg(feature = "auth")]
  #[test]
  fn test_config_derive_missing_auth_field() {
    let input = quote! {
      struct MyConfig {
        #[base]
        base: BaseConfig,
        #[metrics]
        metrics: MetricsConfig,
        #[site]
        site: SiteConfig,
      }
    };
    assert!(
      config(input)
        .to_string()
        .contains("No field with #[auth] attribute found")
    );
  }
}
