use proc_macro2::TokenStream;
use quote::quote;
use syn::{LitByteStr, LitStr};

pub(crate) fn expand(input: TokenStream) -> syn::Result<TokenStream> {
  let literal: LitStr = syn::parse2(input)?;
  if !literal.suffix().is_empty() {
    return Err(syn::Error::new_spanned(literal, "literal suffixes are not supported"));
  }
  let value = literal.value();
  assuan_protocol::Command::parse(value.as_bytes())
    .map_err(|error| return syn::Error::new_spanned(&literal, error))?;
  let protocol = crate::paths::resolve_path("assuan-protocol")?;
  let bytes = LitByteStr::new(value.as_bytes(), literal.span());
  return Ok(quote! {
    #protocol::Command::parse(#bytes)
      .expect("command literal was validated during compilation")
  });
}

#[cfg(test)]
mod tests {
  use quote::quote;

  #[test]
  fn invalid_commands_report_errors_without_echoing_payloads() {
    for value in [
      "GETINFO secret\r",
      "GETINFO secret\n",
      "GETINFO secret\0",
      "",
      "#secret",
      "BAD%TOKEN secret",
      "BAD\tTOKEN secret",
    ] {
      let literal = syn::LitStr::new(value, proc_macro2::Span::call_site());
      let error = super::expand(quote!(#literal)).unwrap_err();
      assert!(!error.to_string().contains("secret"));
      assert!(error.to_compile_error().to_string().contains("compile_error"));
    }
  }

  #[test]
  fn command_requires_one_unsuffixed_string_literal() {
    for input in [
      quote!(name),
      quote!(concat!("GET", "INFO")),
      quote!(b"GETINFO"),
      quote!("GETINFO" "version"),
      quote!("GETINFO"suffix),
      quote!(),
    ] {
      assert!(super::expand(input).is_err());
    }
  }
}
