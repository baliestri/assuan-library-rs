use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::{
  FnArg, GenericArgument, ItemFn, LitStr, PathArguments, ReturnType, Token, Type, parse::Parser,
  punctuated::Punctuated,
};

pub(crate) fn expand_attribute(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
  let metadata = Punctuated::<LitStr, Token![,]>::parse_terminated.parse2(args)?;
  if metadata.len() != 2 {
    return Err(syn::Error::new_spanned(metadata, "expected command name and description"));
  }
  let mut metadata = metadata.iter();
  let name = metadata.next().expect("two literals checked");
  let description = metadata.next().expect("two literals checked");
  validate_metadata(name, description)?;
  let mut function: ItemFn = syn::parse2(item)?;
  let state = validate_signature(&function)?;
  validate_attributes(&function)?;
  let protocol = crate::paths::resolve_path("assuan-protocol")?;
  let server = crate::paths::resolve_path("assuan-server")?;
  let ident = function.sig.ident.clone();
  let visibility = function.vis.clone();
  let adapter_attrs: Vec<_> = function
    .attrs
    .iter()
    .filter(|attribute| {
      return !attribute.path().is_ident("inline")
        && !attribute.path().is_ident("cold")
        && !attribute.path().is_ident("must_use");
    })
    .cloned()
    .collect();
  let gates: Vec<_> = function
    .attrs
    .iter()
    .filter(|attribute| {
      return attribute.path().is_ident("cfg");
    })
    .cloned()
    .collect();
  // The helper lives in this adapter's inherent impl, not the user's module
  // namespace. Mixed-site identifiers also isolate generated local bindings.
  let helper = Ident::new("__assuan_command_body", Span::mixed_site());
  let command = Ident::new("__assuan_command", Span::mixed_site());
  let context = Ident::new("__assuan_context", Span::mixed_site());
  function.sig.ident = helper.clone();
  function.vis = syn::Visibility::Inherited;
  function.attrs.retain(|attribute| {
    return !attribute.path().is_ident("doc")
      && !attribute.path().is_ident("cfg")
      && !attribute.path().is_ident("deprecated");
  });
  return Ok(quote! {
    #(#adapter_attrs)*
    #[allow(non_camel_case_types)]
    #[derive(::core::fmt::Debug)]
    #visibility struct #ident;

    #(#gates)*
    impl #ident {
      #function
    }

    #(#gates)*
    impl #server::Handler<#state> for #ident {
      fn name(&self) -> &str {
        return #name;
      }
      fn description(&self) -> &str {
        return #description;
      }
      fn call<'a>(
        &'a self,
        #command: #protocol::Command<'a>,
        mut #context: #server::CommandContext<'a, #state>,
      ) -> #server::HandlerFuture<'a> {
        return ::std::boxed::Box::pin(async move {
          return Self::#helper(#command, &mut #context).await;
        });
      }
    }
  });
}

fn validate_metadata(name: &LitStr, description: &LitStr) -> syn::Result<()> {
  for literal in [name, description] {
    if !literal.suffix().is_empty() {
      return Err(syn::Error::new_spanned(literal, "literal suffixes are not supported"));
    }
  }
  let wire_name = name.value();
  assuan_protocol::Command::new(&wire_name, b"")
    .map_err(|error| return syn::Error::new_spanned(name, error))?;
  if wire_name.len() >= assuan_protocol::MAX_LINE_BYTES {
    return Err(syn::Error::new_spanned(name, "command name exceeds the wire limit"));
  }
  if matches!(wire_name.as_str(), "NOP" | "BYE" | "HELP" | "RESET" | "OPTION") {
    return Err(syn::Error::new_spanned(name, "built-in command names are reserved"));
  }
  if description.value().bytes().any(|byte| return matches!(byte, 0 | b'\r' | b'\n')) {
    return Err(syn::Error::new_spanned(description, "description must not contain NUL, CR or LF"));
  }
  return Ok(());
}

fn validate_signature(function: &ItemFn) -> syn::Result<Type> {
  let signature = &function.sig;
  if signature.asyncness.is_none()
    || signature.constness.is_some()
    || signature.unsafety.is_some()
    || signature.abi.is_some()
    || signature.variadic.is_some()
    || !signature.generics.params.is_empty()
    || signature.generics.where_clause.is_some()
  {
    return Err(syn::Error::new_spanned(
      signature,
      "expected a safe async function without generics, a where clause, or an extern ABI",
    ));
  }
  if signature.inputs.len() != 2 {
    return Err(syn::Error::new_spanned(
      &signature.inputs,
      "expected Command and &mut CommandContext arguments",
    ));
  }
  let mut inputs = signature.inputs.iter();
  let first = argument_type(inputs.next().expect("two arguments checked"))?;
  let command = named_type(first, "Command")?;
  let (lifetime, types) = type_arguments(command)?;
  validate_borrow_lifetime(lifetime)?;
  if !types.is_empty() {
    return Err(syn::Error::new_spanned(first, "Command accepts only its borrow lifetime"));
  }
  let second = argument_type(inputs.next().expect("two arguments checked"))?;
  let Type::Reference(reference) = second else {
    return Err(syn::Error::new_spanned(second, "expected &mut CommandContext"));
  };
  if reference.mutability.is_none() {
    return Err(syn::Error::new_spanned(second, "context must be borrowed mutably"));
  }
  validate_borrow_lifetime(reference.lifetime.as_ref())?;
  let context = named_type(&reference.elem, "CommandContext")?;
  let (lifetime, types) = type_arguments(context)?;
  validate_borrow_lifetime(lifetime)?;
  if types.len() > 1 {
    return Err(syn::Error::new_spanned(second, "expected at most one concrete state type"));
  }
  let ReturnType::Type(_, output) = &signature.output else {
    return Err(syn::Error::new_spanned(&signature.output, "expected Result<(), HandlerError>"));
  };
  let result = named_type(output, "Result")?;
  let (lifetime, result_types) = type_arguments(result)?;
  if lifetime.is_some()
    || result_types.len() != 2
    || !matches!(result_types[0], Type::Tuple(tuple) if tuple.elems.is_empty())
  {
    return Err(syn::Error::new_spanned(output, "expected Result<(), HandlerError>"));
  }
  let error = named_type(result_types[1], "HandlerError")?;
  if !matches!(error.arguments, PathArguments::None) {
    return Err(syn::Error::new_spanned(error, "HandlerError has no generic arguments"));
  }
  return Ok(
    types.first().map_or_else(|| return syn::parse_quote!(()), |value| return (*value).clone()),
  );
}

fn argument_type(argument: &FnArg) -> syn::Result<&Type> {
  match argument {
    FnArg::Typed(value) => return Ok(&value.ty),
    FnArg::Receiver(_) => {
      return Err(syn::Error::new_spanned(argument, "receivers are not supported"));
    }
  }
}

fn named_type<'a>(value: &'a Type, expected: &str) -> syn::Result<&'a syn::PathSegment> {
  if let Type::Path(path) = value
    && path.qself.is_none()
    && let Some(last) = path.path.segments.last()
    && last.ident == expected
  {
    return Ok(last);
  }
  return Err(syn::Error::new_spanned(value, format!("expected {expected}")));
}

fn type_arguments(segment: &syn::PathSegment) -> syn::Result<(Option<&syn::Lifetime>, Vec<&Type>)> {
  let mut lifetime = None;
  let mut types = Vec::new();
  match &segment.arguments {
    PathArguments::None => {}
    PathArguments::AngleBracketed(arguments) => {
      for argument in &arguments.args {
        match argument {
          GenericArgument::Lifetime(value) if lifetime.is_none() && types.is_empty() => {
            lifetime = Some(value);
          }
          GenericArgument::Type(value) => types.push(value),
          _ => return Err(syn::Error::new_spanned(argument, "unsupported type argument")),
        }
      }
    }
    PathArguments::Parenthesized(_) => {
      return Err(syn::Error::new_spanned(segment, "unsupported type arguments"));
    }
  }
  return Ok((lifetime, types));
}

fn validate_borrow_lifetime(lifetime: Option<&syn::Lifetime>) -> syn::Result<()> {
  if let Some(lifetime) = lifetime
    && lifetime.ident != "_"
  {
    return Err(syn::Error::new_spanned(lifetime, "use an elided lifetime or '_ for call borrows"));
  }
  return Ok(());
}

fn validate_attributes(function: &ItemFn) -> syn::Result<()> {
  for attribute in &function.attrs {
    if ![
      "doc",
      "cfg",
      "allow",
      "warn",
      "deny",
      "forbid",
      "deprecated",
      "inline",
      "cold",
      "must_use",
    ]
    .iter()
    .any(|name| return attribute.path().is_ident(name))
    {
      return Err(syn::Error::new_spanned(
        attribute,
        "unsupported function attribute; apply conditional attributes outside assuan_command",
      ));
    }
  }
  return Ok(());
}

#[cfg(test)]
mod tests {
  use quote::quote;

  fn valid_function() -> proc_macro2::TokenStream {
    return quote! {
      async fn echo(command: Command<'_>, context: &mut CommandContext<'_>)
        -> Result<(), HandlerError> {
        return context.send_data(command.args()).await;
      }
    };
  }

  #[test]
  fn private_functions_default_to_unit_state_and_keep_helpers_private() {
    let file: syn::File =
      syn::parse2(super::expand_attribute(quote!("ECHO", ""), valid_function()).unwrap()).unwrap();
    let syn::Item::Struct(adapter) = &file.items[0] else {
      panic!("expected adapter");
    };
    assert!(matches!(adapter.vis, syn::Visibility::Inherited));
    assert!(adapter.attrs.iter().any(|attr| return attr.path().is_ident("allow")));
    let syn::Item::Impl(helper_impl) = &file.items[1] else {
      panic!("expected inherent impl");
    };
    let syn::ImplItem::Fn(helper) = &helper_impl.items[0] else {
      panic!("expected helper");
    };
    assert!(matches!(helper.vis, syn::Visibility::Inherited));
    assert!(helper.sig.asyncness.is_some());
    let syn::Item::Impl(handler) = &file.items[2] else {
      panic!("expected Handler impl");
    };
    assert_eq!(handler.trait_.as_ref().unwrap().1.segments.last().unwrap().ident, "Handler");
    assert!(quote!(#handler).to_string().contains("Handler < () >"));
    assert!(handler.attrs.is_empty());
  }

  #[test]
  fn invalid_signatures_produce_compile_errors() {
    for item in [
      quote!(
        fn handler(c: Command<'_>, ctx: &mut CommandContext<'_>) -> Result<(), HandlerError> {}
      ),
      quote!(
        async unsafe fn handler(
          c: Command<'_>,
          ctx: &mut CommandContext<'_>,
        ) -> Result<(), HandlerError> {
        }
      ),
      quote!(
        async extern "C" fn handler(
          c: Command<'_>,
          ctx: &mut CommandContext<'_>,
        ) -> Result<(), HandlerError> {
        }
      ),
      quote!(
        async fn handler<T>(
          c: Command<'_>,
          ctx: &mut CommandContext<'_, T>,
        ) -> Result<(), HandlerError> {
        }
      ),
      quote!(
        async fn handler<'a>(
          c: Command<'a>,
          ctx: &mut CommandContext<'a>,
        ) -> Result<(), HandlerError> {
        }
      ),
      quote!(
        async fn handler(c: Command<'_>, ctx: &mut CommandContext<'_>)
        where (): Send {
        }
      ),
      quote!(
        async fn handler(&self, ctx: &mut CommandContext<'_>) -> Result<(), HandlerError> {}
      ),
      quote!(
        async fn handler(
          c: Command<'_>,
          ctx: &mut CommandContext<'_>,
          extra: (),
        ) -> Result<(), HandlerError> {
        }
      ),
      quote!(
        async fn handler(c: Command<'_>, ...) -> Result<(), HandlerError> {}
      ),
      quote!(
        async fn handler(
          c: &Command<'_>,
          ctx: &mut CommandContext<'_>,
        ) -> Result<(), HandlerError> {
        }
      ),
      quote!(
        async fn handler(
          c: Command<'static>,
          ctx: &mut CommandContext<'_>,
        ) -> Result<(), HandlerError> {
        }
      ),
      quote!(
        async fn handler(c: Command<'_>, ctx: CommandContext<'_>) -> Result<(), HandlerError> {}
      ),
      quote!(
        async fn handler(c: Command<'_>, ctx: &CommandContext<'_>) -> Result<(), HandlerError> {}
      ),
      quote!(
        async fn handler(c: Command<'_>, ctx: &mut CommandContext<'_>) {}
      ),
      quote!(
        async fn handler(
          c: Command<'_>,
          ctx: &mut CommandContext<'_>,
        ) -> Result<bool, HandlerError> {
        }
      ),
      quote!(
        async fn handler(c: Command<'_>, ctx: &mut CommandContext<'_>) -> Result<(), OtherError> {}
      ),
      quote!(
        async fn handler(
          c: Command<'_>,
          ctx: &mut CommandContext<'_>,
        ) -> Result<(), HandlerError<()>> {
        }
      ),
      quote!(
        struct Handler;
      ),
    ] {
      let error = super::expand_attribute(quote!("ECHO", ""), item).unwrap_err();
      assert!(error.to_compile_error().to_string().contains("compile_error"));
    }
  }

  #[test]
  fn metadata_rejects_reserved_names_delimiters_and_invalid_literals() {
    for name in ["NOP", "BYE", "HELP", "RESET", "OPTION", "", "BAD NAME", "BAD%NAME", "#comment"] {
      let name = syn::LitStr::new(name, proc_macro2::Span::call_site());
      assert!(super::expand_attribute(quote!(#name, ""), valid_function()).is_err());
    }
    for description in ["private\nvalue", "private\rvalue", "private\0value"] {
      let description = syn::LitStr::new(description, proc_macro2::Span::call_site());
      let error =
        super::expand_attribute(quote!("ECHO", #description), valid_function()).unwrap_err();
      assert!(!error.to_string().contains("private"));
    }
    for args in [
      quote!(),
      quote!("ECHO"),
      quote!("ECHO", "", ""),
      quote!(name, ""),
      quote!(b"ECHO", ""),
      quote!("ECHO"suffix, ""),
    ] {
      assert!(super::expand_attribute(args, valid_function()).is_err());
    }
    let long = syn::LitStr::new(
      &"A".repeat(assuan_protocol::MAX_LINE_BYTES),
      proc_macro2::Span::call_site(),
    );
    assert!(super::expand_attribute(quote!(#long, ""), valid_function()).is_err());
    assert!(super::expand_attribute(quote!("nop", "",), valid_function()).is_ok());
  }

  #[test]
  fn conditional_items_gate_adapter_and_both_impls() {
    let item = valid_function();
    let output =
      super::expand_attribute(quote!("ECHO", ""), quote!(#[cfg(feature = "example")] #item))
        .unwrap();
    let file: syn::File = syn::parse2(output).unwrap();
    assert_eq!(file.items.len(), 3);
    for item in file.items {
      let attributes = match item {
        syn::Item::Struct(value) => value.attrs,
        syn::Item::Impl(value) => value.attrs,
        _ => panic!("unexpected generated item"),
      };
      assert!(attributes.iter().any(|attr| return attr.path().is_ident("cfg")));
    }
  }

  #[test]
  fn unsupported_transforming_attributes_are_rejected() {
    let item = valid_function();
    for attribute in [
      quote!(#[other_macro]),
      quote!(#[cfg_attr(feature = "example", inline)]),
      quote!(#[expect(unused_variables)]),
    ] {
      assert!(super::expand_attribute(quote!("ECHO", ""), quote!(#attribute #item)).is_err());
    }
  }

  #[test]
  fn expansion_preserves_metadata_visibility_and_handler_state() {
    let output = super::expand_attribute(
      quote!("GETINFO", "Returns server information"),
      quote! {
        /// Retrieves public information.
        pub async fn get_info(command: Command<'_>, context: &mut CommandContext<'_, State>)
          -> Result<(), HandlerError> {
          return context.send_data(command.args()).await;
        }
      },
    )
    .unwrap();
    let file = syn::parse2::<syn::File>(output.clone()).unwrap();
    assert!(file.items.iter().any(|item| {
      return matches!(item, syn::Item::Struct(value)
      if value.ident == "get_info" && matches!(value.vis, syn::Visibility::Public(_)));
    }));
    let text = output.to_string();
    assert!(text.contains("GETINFO"));
    assert!(text.contains("Returns server information"));
    assert!(text.contains("Retrieves public information"));
    assert!(text.contains("Handler < State >"));
  }
}
