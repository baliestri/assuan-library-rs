# assuan-library

A facade for asynchronous Assuan clients and servers, bounded protocol
primitives, protected data, custom transports and validated literals.
The transport layer uses Tokio; the facade requires std.

## Features

| Feature | Available API |
| --- | --- |
| Always | `protocol`, `transport`, commands, protected buffers, endpoints, streams and discovery |
| `client` | `Client`, transactions, inquiries, bounded response collection |
| `server` | `Server`, sessions, handlers, registry and hooks |
| `sexpr` | Canonical S-expression parsing and encoding |
| `macros` | `command!`; `assuan_command` also needs server, `sexpr!` also needs sexpr |

All four features are enabled by default. Enabling macros does not enable
client or server. The macro crate uses the pure parsers during compilation;
that does not expose the optional S-expression API in the facade.

```toml
[dependencies]
assuan-library = { version = "0.1.0", default-features = false, features = ["client"] }
tokio = { version = "1", features = ["macros", "rt", "net", "time"] }
```

No package has been published yet. Inside a checkout, use a Cargo path
dependency on `crates/assuan-library`. Types are reexports of the component
crates, so mixing the facade with direct dependencies preserves type identity.

## Public commands and protected buffers

```rust
use assuan_library::{Command, SecretBytes};

let command = Command::new("GETINFO", b"version")?;
assert_eq!(command.args(), b"version");
let mut secret = SecretBytes::with_capacity(64)?;
secret.extend_from_slice(b"example")?;
assert_eq!(secret.expose(), b"example");
# Ok::<(), Box<dyn std::error::Error>>(())
```

Commands and streaming events borrow their inputs. A transaction exclusively
borrows its client, and each event borrows its transaction until released.
Dropping unfinished operations closes the connection. Secret types do not
implement Clone or Copy; exposure is explicit. Buffer erasure cannot clear
caller copies, OS buffers, swap or crash dumps.

## Explicit server handlers

```rust
# #[cfg(all(feature = "server", feature = "macros"))]
# {
use assuan_library::{assuan_command, Command, CommandContext, HandlerError, Server};

#[assuan_command("ECHO", "Returns public arguments")]
async fn echo(command: Command<'_>, context: &mut CommandContext<'_>) -> Result<(), HandlerError> {
  return context.send_data(command.args()).await;
}

let mut server = Server::new(|| (), Default::default());
server.register(echo).unwrap();
# }
```

The attribute replaces the function name with a registrable unit value
implementing `Handler<S>`; its body becomes a private associated async helper.
Visibility and documentation are preserved. The helper borrows context and
command bytes across awaits; each call allocates one boxed Send future.
Use `CommandContext<'_, MyState>` for concrete session state. Omitted state
means `()`. State needs Send, not Sync. Ordinary `Handler` implementations
and the `handler` function can share the same registry and sessions.

The function must be safe, async, nongeneric, take two arguments, and return
`Result<(), HandlerError>`. Type paths may be qualified but signature type
aliases are not recognized. Names NOP, BYE, HELP, RESET and OPTION are reserved.
Duplicate names are rejected by registration. See `assuan-macros` for
supported attributes and syntax. Handlers leave final OK/ERR to the session.

## Transports and discovery

TCP is available on every supported platform; Unix sockets are available
on Unix and named pipes on Windows. TCP supplies no OS-authenticated peer
identity. Custom streams implement Tokio `AsyncRead`/`AsyncWrite` plus Send and
Unpin, then enter through `Stream::new` and `Client::from_stream`. A custom
`Acceptor` may leave its endpoint at None. Transport implementations own
their wakeup, short-I/O and cancellation behavior.

`AgentLocator` uses gpgconf to discover an existing agent and never launches
one. `ResolvedEndpoint::connect` applies any native nonce handshake before
`Client::from_stream`. Discovery errors remain typed and redacted.

## Examples

Run from the workspace root:

- `cargo run -p assuan-library --example tcp_server -- 9000`
- `cargo run -p assuan-library --example tcp_client -- 9000`
- `cargo run -p assuan-library --example custom_transport`
- `cargo run -p assuan-library --example agent_info -- gpgconf [homedir]`

TCP examples explicitly use 127.0.0.1 and accept a port argument. Stop the
server with Ctrl+C. The custom example runs its own byte adapter, acceptor,
client and server in memory, awaits a final response and shuts down without
external services. The agent example requires an already-running agent.

Servers default to 128 concurrent sessions and a shared 30-second shutdown
grace period, after which remaining tasks are aborted and joined. Configure
operation deadlines, inquiry limits and collection limits for the application.
Diagnostics omit payloads; the examples print completion and byte counts.
Independent interoperability with real `GnuPG` remains a separate test stage.
