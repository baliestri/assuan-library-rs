# assuan-macros

Compile-time validated literals for Assuan commands and canonical
S-expressions. This procedural macro crate depends on the pure protocol and
S-expression crates, without a production dependency on the server or facade.

## Command literals

Add `assuan-macros` and `assuan-protocol` as dependencies:

```rust
use assuan_macros::command;

let command: assuan_protocol::Command<'static> = command!("GETINFO version");
assert_eq!(command.name(), "GETINFO");
assert_eq!(command.args(), b"version");
```

`command!` accepts exactly one Rust string literal, including raw strings.
It borrows static bytes and preserves argument spaces, UTF-8 and percent
escapes. The shared parser validates the literal at compile time; the
generated expression calls the same safe parser again without allocation.
It is not a const constructor. Wire framing still enforces line length.

The generated path prefers `assuan-library::protocol` when the facade is
present, otherwise the direct protocol dependency. Cargo dependency aliases
are supported. Using the macro inside the defining library resolves through
`crate`; integration targets in that same package must provide the matching
crate-root reexports. Full facade consumer tests are planned with the facade.

## S-expression literals

```rust
use assuan_macros::sexpr;

const VALUE: &[u8] = sexpr!(("hash" ("sha256" b"\x00\xff") ""));
assert_eq!(VALUE, b"(4:hash(6:sha2562:\x00\xff)0:)");
```

Atoms are Rust string or byte-string literals; lists use parentheses and
whitespace, without commas. Raw strings, binary bytes, empty atoms and empty
lists are supported. One root expression is required; dynamic expressions
and nested macro invocations are rejected. String lengths count UTF-8 bytes.

Expansion builds bounded canonical bytes and validates them using
`assuan_sexpr::parse_complete`. Default shared limits apply: 16 MiB encoded
input, 16 MiB per atom, 65,536 nodes and 64 nested lists. The result is a
`&'static [u8]` usable in const/static contexts with no runtime allocation or
parsing. Using `sexpr!` needs no runtime S-expression dependency.

## Security and diagnostics

These macros embed literal data in the executable. Use them for public
protocol constants, never for credentials or other secrets requiring erasure.
Library-generated validation messages do not include payloads, but compiler
diagnostics can display source lines. Invalid input produces compilation
errors rather than application panics.
