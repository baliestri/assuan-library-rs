use assuan_sexpr::{ParseLimits, parse_complete};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{
  LitByteStr, LitStr,
  parse::{Parse, ParseStream},
};

struct Literal {
  bytes: Vec<u8>,
}

impl Parse for Literal {
  fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
    let mut builder = Builder {
      bytes: Vec::new(),
      nodes: 0,
      limits: ParseLimits::default(),
    };
    builder.expression(input, 0)?;
    return Ok(Self {
      bytes: builder.bytes,
    });
  }
}

struct Builder {
  bytes: Vec<u8>,
  nodes: usize,
  limits: ParseLimits,
}

impl Builder {
  fn expression(&mut self, input: ParseStream<'_>, depth: usize) -> syn::Result<()> {
    if self.nodes >= self.limits.max_nodes() {
      return Err(input.error("S-expression node limit exceeded"));
    }
    self.nodes += 1;
    if input.peek(syn::token::Paren) {
      if depth >= self.limits.max_depth() {
        return Err(input.error("S-expression nesting limit exceeded"));
      }
      let content;
      let parens = syn::parenthesized!(content in input);
      self.append(b"(", parens.span.open())?;
      while !content.is_empty() {
        self.expression(&content, depth + 1)?;
      }
      self.append(b")", parens.span.close())?;
    } else if input.peek(LitStr) {
      let literal: LitStr = input.parse()?;
      if !literal.suffix().is_empty() {
        return Err(syn::Error::new_spanned(literal, "literal suffixes are not supported"));
      }
      self.atom(literal.value().as_bytes(), literal.span())?;
    } else if input.peek(LitByteStr) {
      let literal: LitByteStr = input.parse()?;
      if !literal.suffix().is_empty() {
        return Err(syn::Error::new_spanned(literal, "literal suffixes are not supported"));
      }
      self.atom(&literal.value(), literal.span())?;
    } else {
      return Err(input.error("expected a string, byte string, or parenthesized list"));
    }
    return Ok(());
  }

  fn atom(&mut self, bytes: &[u8], span: Span) -> syn::Result<()> {
    if bytes.len() > self.limits.max_atom() {
      return Err(syn::Error::new(span, "S-expression atom limit exceeded"));
    }
    self.append(bytes.len().to_string().as_bytes(), span)?;
    self.append(b":", span)?;
    return self.append(bytes, span);
  }

  fn append(&mut self, bytes: &[u8], span: Span) -> syn::Result<()> {
    if bytes.len() > self.limits.max_input().saturating_sub(self.bytes.len()) {
      return Err(syn::Error::new(span, "S-expression encoded size limit exceeded"));
    }
    self
      .bytes
      .try_reserve(bytes.len())
      .map_err(|_| return syn::Error::new(span, "cannot allocate S-expression output"))?;
    self.bytes.extend_from_slice(bytes);
    return Ok(());
  }
}

pub(crate) fn expand(input: TokenStream) -> syn::Result<TokenStream> {
  let literal: Literal = syn::parse2(input.clone())?;
  parse_complete(&literal.bytes, ParseLimits::default())
    .map_err(|error| return syn::Error::new_spanned(input, error))?;
  let bytes = LitByteStr::new(&literal.bytes, Span::call_site());
  return Ok(quote!(#bytes as &'static [u8]));
}

#[cfg(test)]
mod tests {
  use proc_macro2::{Delimiter, Group, TokenStream};
  use quote::quote;

  #[test]
  fn dynamic_or_unsupported_expressions_are_rejected() {
    for input in [
      quote!(("key" variable)),
      quote!(concat!("a", "b")),
      quote!((42)),
      quote!((b'x')),
      quote!(("a", "b")),
      quote!(),
      quote!("a" "b"),
      quote!("a"suffix),
      quote!(b"a"suffix),
      quote!(["a"]),
    ] {
      assert!(super::expand(input).is_err());
    }
  }

  #[test]
  fn incomplete_lists_are_rejected_by_tokenization() {
    assert!("(\"atom\"".parse::<TokenStream>().is_err());
  }

  #[test]
  fn nesting_limit_is_checked_before_recursing_further() {
    let mut input = quote!("atom");
    for _ in 0..assuan_sexpr::ParseLimits::default().max_depth() {
      input =
        TokenStream::from(proc_macro2::TokenTree::Group(Group::new(Delimiter::Parenthesis, input)));
    }
    assert!(super::expand(input.clone()).is_ok());
    let excessive =
      TokenStream::from(proc_macro2::TokenTree::Group(Group::new(Delimiter::Parenthesis, input)));
    assert!(super::expand(excessive).unwrap_err().to_string().contains("nesting limit"));
  }

  #[test]
  fn construction_enforces_node_atom_and_encoded_size_limits() {
    use syn::parse::Parser;
    let limits = assuan_sexpr::ParseLimits::new(8, 4, 3, 2).unwrap();
    for input in [quote!(("" "" "")), quote!("12345"), quote!(("1234" ""))] {
      let parser = |stream: syn::parse::ParseStream<'_>| {
        let mut builder = super::Builder {
          bytes: Vec::new(),
          nodes: 0,
          limits,
        };
        return builder.expression(stream, 0);
      };
      assert!(parser.parse2(input).is_err());
    }
  }
}
