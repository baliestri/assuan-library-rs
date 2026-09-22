# assuan-library-rs

A Rust implementation of the [Assuan protocol](https://gnupg.org/documentation/manuals/assuan/) with asynchronous clients, servers and extensible transports.

## Project status

All seven crates are implemented. The facade exposes borrowed protocol types, protected buffers, streaming client transactions and inquiries, typed server handlers and hooks, bounded concurrent serving, canonical S-expressions, and compile-validated macros. TCP, Unix sockets, Windows named pipes and application-defined byte transports are supported on their respective platforms. Native agent discovery does not launch services. Independent interoperability tests with real GnuPG pass on Windows, Linux and macOS. No package has been published.

## Workspace design

| Crate | Responsibility |
| --- | --- |
| `assuan-client` | Asynchronous client, streaming transactions and inquiries |
| `assuan-library` | Public facade with optional client, server, macros and S-expression features |
| `assuan-macros` | Procedural macros for literals and explicit command handlers |
| `assuan-protocol` | Protocol parsing, encoding and state machines without I/O |
| `assuan-server` | Sessions, command dispatch and inquiries |
| `assuan-sexpr` | Canonical S-expression parsing and encoding |
| `assuan-transport` | TCP, Unix domain sockets, Windows named pipes and endpoint discovery |

## Design goals

- Async I/O with Tokio and a protocol core independent of the runtime.
- TCP on Windows, Linux and macOS; Unix domain sockets on Linux and macOS; named pipes on Windows.
- Borrowed data where practical, explicit ownership and bounded buffering.
- Dedicated secret types, redacted diagnostics and cleanup of sensitive buffers.
- No unsafe code in this workspace.
- Documented public APIs, regression tests and independent interoperability checks.

## Toolchain

The workspace uses Rust 1.98.1 and edition 2024. Formatting requires nightly
to apply every option in rustfmt.toml:

```text
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

## Getting started

Use a path dependency on `crates/assuan-library` until publication. The facade
enables `client`, `server`, `macros` and `sexpr` by default; each can be
selected independently with default features disabled. Protocol and transport
remain available. Macros do not implicitly enable client or server.

See [the facade guide](crates/assuan-library/README.md) for handler syntax,
feature selection and ownership contracts. Direct crate dependencies remain
supported. Macro and ordinary handlers can share a session's concrete state.

```text
cargo run -p assuan-library --example custom_transport
cargo run -p assuan-library --example tcp_server -- 9000
cargo run -p assuan-library --example tcp_client -- 9000
```

The custom example is self-contained. Run the TCP server and client in
separate terminals; both explicitly use loopback. The agent discovery example
requires an existing agent and accepts a gpgconf path and optional homedir.

Compilation tests cover invalid macros, exclusive transaction/event borrows,
non-cloneable secrets and Send requirements. The renamed-consumer fixture
checks Cargo aliases:

```text
cargo test -p assuan-library --all-features
cargo check --manifest-path crates/assuan-library/tests/fixtures/renamed/Cargo.toml
```

## Security and package verification

Read the [security and ownership guide](docs/security.md) before exchanging secrets.
The [publishing guide](docs/publishing.md) describes local verification of all seven
archives and the prerequisites for a future release. Run `./scripts/verify-packages.ps1`
from a clean checkout after approved commits; it does not publish packages.
See [CI coverage](docs/continuous-integration.md) and
[protocol compatibility](docs/protocol-compatibility.md) for the tested matrix.

## License

MIT. See [LICENSE.md](LICENSE.md).
