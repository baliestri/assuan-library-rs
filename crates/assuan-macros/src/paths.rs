use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream};
use quote::quote;

pub(crate) fn resolve_path(package: &str) -> syn::Result<TokenStream> {
  return select_path(package, crate_name("assuan-library").ok(), crate_name(package).ok());
}

fn select_path(
  package: &str,
  facade: Option<FoundCrate>,
  direct: Option<FoundCrate>,
) -> syn::Result<TokenStream> {
  if let Some(found) = facade {
    let root = root_path(found)?;
    let module = match package {
      "assuan-protocol" => quote!(protocol),
      "assuan-server" => quote!(server),
      "assuan-sexpr" => quote!(sexpr),
      _ => return Err(syn::Error::new(Span::call_site(), "unsupported Assuan package")),
    };
    return Ok(quote!(#root::#module));
  }
  if let Some(found) = direct {
    return root_path(found);
  }
  return Err(syn::Error::new(
    Span::call_site(),
    format!("add a dependency on {package} or assuan-library to use this macro"),
  ));
}

fn root_path(found: FoundCrate) -> syn::Result<TokenStream> {
  match found {
    FoundCrate::Itself => return Ok(quote!(crate)),
    FoundCrate::Name(name) => {
      let ident = syn::parse_str::<syn::Ident>(&name)
        .or_else(|_| return syn::parse_str::<syn::Ident>(&format!("r#{name}")))?;
      return Ok(quote!(::#ident));
    }
  }
}

#[cfg(test)]
mod tests {
  use proc_macro_crate::FoundCrate::{Itself, Name};

  #[test]
  fn renamed_facade_is_preferred_over_direct_dependency() {
    let path = super::select_path(
      "assuan-protocol",
      Some(Name("library_alias".into())),
      Some(Name("protocol_alias".into())),
    )
    .unwrap();
    assert_eq!(path.to_string(), ":: library_alias :: protocol");
  }

  #[test]
  fn direct_dependency_uses_its_cargo_name() {
    let path = super::select_path("assuan-protocol", None, Some(Name("wire".into()))).unwrap();
    assert_eq!(path.to_string(), ":: wire");
    let keyword = super::select_path("assuan-protocol", None, Some(Name("type".into()))).unwrap();
    assert_eq!(keyword.to_string(), ":: r#type");
  }

  #[test]
  fn itself_resolves_to_crate_without_an_external_import() {
    assert_eq!(
      super::select_path("assuan-protocol", None, Some(Itself)).unwrap().to_string(),
      "crate"
    );
    assert_eq!(
      super::select_path("assuan-protocol", Some(Itself), None).unwrap().to_string(),
      "crate :: protocol"
    );
  }

  #[test]
  fn missing_dependencies_are_diagnostic_errors() {
    assert!(super::select_path("assuan-protocol", None, None).is_err());
    assert!(super::select_path("other", Some(Itself), None).is_err());
  }
}
