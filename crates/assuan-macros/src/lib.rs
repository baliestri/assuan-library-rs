#![doc = include_str!("../README.md")]

mod command;
mod paths;
mod sexpr;

use proc_macro::TokenStream;

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
