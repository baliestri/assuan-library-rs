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
are supported. The facade declares an explicit self-alias so macros also work
in its own library, examples and doctests. Direct defining crates resolve
through `crate`. Consumers do not need to import private helper paths.

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

## Explicit command handlers

`#[assuan_command("ECHO", "Returns arguments")]` turns a safe async function
into a unit value implementing `assuan_server::Handler<S>`. Add a direct
`assuan-server` dependency alongside the protocol and macro crates:

```rust
use assuan_macros::assuan_command;
use assuan_protocol::Command;
use assuan_server::{CommandContext, HandlerError, Registry};

#[assuan_command("ECHO", "Returns arguments")]
/// Returns the original command arguments.
pub async fn echo(
  command: Command<'_>,
  context: &mut CommandContext<'_>,
) -> Result<(), HandlerError> {
  return context.send_data(command.args()).await;
}

let mut registry = Registry::<()>::new();
registry.register(echo)?;
# Ok::<(), assuan_server::RegistryError>(())
```

The original name denotes a registrable adapter, not a callable function.
Visibility and rustdoc stay on that adapter; the body becomes a private
associated helper. The macro receives context by value and lends it to the
helper. Command arguments can remain borrowed across awaits; each invocation
allocates one boxed Send future. No payload copy or automatic registration is
introduced. Final OK/ERR responses remain the session runner's responsibility.

Use `&mut CommandContext<'_, MyState>` for concrete session state; omission
means `()`. State must be Send and static, but need not be Sync. Functions
must have exactly two arguments and return `Result<(), HandlerError>`.
Qualified paths are accepted; aliases of `Command`, `CommandContext`, `Result` or
`HandlerError` are not recognized syntactically. Use elided or anonymous call
lifetimes. Generic functions, receivers, extern ABIs, unsafe functions and
variadics are rejected. Rust checks the body's types and Send requirements.

Names obey protocol validation and the wire length limit. NOP, BYE, HELP,
RESET and OPTION are reserved, case-sensitively. Descriptions may be empty or
Unicode but cannot contain NUL, CR or LF. Duplicate registration remains a
runtime registry error.

Supported inner attributes are rustdoc, `cfg`, `allow`/`warn`/`deny`/`forbid`,
`deprecated`, `inline`, `cold` and `must_use`. Function-only attributes stay on the
helper; `cfg` gates all generated items. Other transforming attributes and
`cfg_attr` must be applied outside `assuan_command` so they run first; inner
`expect` is rejected rather than duplicated across generated items. The naming
lint exception is limited to the generated adapter type.

The generated implementation uses std and resolves server/protocol paths
through the facade when present, otherwise through direct dependencies.
Direct consumers are compiled in this crate's tests; facade consumers,
renamed dependencies and compile-fail contracts are tested by assuan-library.
