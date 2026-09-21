#![doc = include_str!("../README.md")]

mod attribute;
mod command;
mod paths;
mod sexpr;

use proc_macro::TokenStream;

/// Turns an async command function into a unit value implementing `Handler<S>`.
///
/// Accepts two string literals: the wire command name and public HELP text.
/// The function must take `Command<'_>` and `&mut CommandContext<'_, S>` and
/// return `Result<(), HandlerError>`. Omitting S means unit state. Use concrete
/// state and elided or anonymous call lifetimes; generic functions, receivers,
/// unsafe functions and extern ABIs are rejected. Qualified type paths work;
/// aliases for the three signature types are not recognized.
///
/// The original name becomes a registrable unit value, retaining visibility
/// and rustdoc. The body moves into a private associated helper. Each call
/// boxes one Send future which borrows the command and context across awaits.
/// The compiler checks concrete state bounds and the body's Send requirement.
/// Register explicitly with `Server::register` or `Registry::register`;
/// duplicate command names remain registration errors.
///
/// Requires the facade with server support or direct protocol and server
/// dependencies; Cargo aliases are supported. The generated code uses std.
/// See the crate documentation for supported function attributes.
///
/// # Errors
/// Emits compiler errors for invalid signatures, reserved or invalid command
/// names, unsupported attributes, and NUL/CR/LF in descriptions. Names must
/// fit the protocol wire limit. Metadata is public and embedded in the binary.
#[proc_macro_attribute]
pub fn assuan_command(args: TokenStream, item: TokenStream) -> TokenStream {
  return attribute::expand_attribute(args.into(), item.into())
    .unwrap_or_else(|error| return error.to_compile_error())
    .into();
}

/// Parses a string literal as a validated, borrowed Assuan command.
///
/// The result is `Command<'static>`. Requires a direct dependency on
/// `assuan-protocol`, or the `assuan-library` facade; renamed dependencies
/// are supported and the facade is preferred when present.
/// Arguments retain their original spaces, UTF-8 and percent escapes.
/// The protocol parser validates at compile time and again without allocation
/// at runtime. This is an expression, not a const constructor.
///
/// # Errors
/// Emits a compiler error for anything other than one string literal, an
/// invalid command token, or NUL/CR/LF. Like `Command::parse`, this validates
/// syntax; framing enforces wire length separately.
///
/// # Security
/// Literal bytes are embedded in the program and cannot be erased. Do not
/// put secrets in literals. Diagnostics omit literal contents, although the
/// compiler may display the corresponding source line.
#[proc_macro]
pub fn command(input: TokenStream) -> TokenStream {
  return command::expand(input.into())
    .unwrap_or_else(|error| return error.to_compile_error())
    .into();
}

/// Encodes a literal S-expression into a static canonical byte slice.
///
/// Accepts one string, byte string, or parenthesized list of expressions.
/// List elements are separated by whitespace, without commas. Raw strings,
/// empty atoms/lists, nested lists and binary atoms are supported. Strings
/// use UTF-8 byte lengths. The result is `&'static [u8]`, usable in const
/// and static declarations, with no runtime parsing or allocation.
///
/// # Errors
/// Emits a compiler error for dynamic expressions, unsupported literals,
/// trailing tokens, or limits exceeded. The shared parser's default limits
/// apply: 16 MiB encoded bytes, 16 MiB per atom, 65,536 nodes and 64 nested
/// lists. Construction is bounded before shared semantic validation.
///
/// # Security
/// Literal bytes are embedded in the program and cannot be erased. Do not
/// put secrets in literals. Compiler diagnostics may show source lines.
#[proc_macro]
pub fn sexpr(input: TokenStream) -> TokenStream {
  return sexpr::expand(input.into())
    .unwrap_or_else(|error| return error.to_compile_error())
    .into();
}
