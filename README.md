# assuan-library-rs

A Rust library planned for the [Assuan protocol](https://gnupg.org/documentation/manuals/assuan/), supporting custom clients and servers and interoperability with GnuPG/gpg-agent.

## Project status

The workspace currently provides bounded canonical S-expression parsing and encoding in `assuan-sexpr`, and protected buffers, wire parsing, framing, encoding, and pure session state machines in `assuan-protocol`, plus TCP, Unix sockets, Windows named pipes, and custom asynchronous streams in `assuan-transport`. The remaining crates and asynchronous protocol integration are under development. No package has been published.

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

The workspace is configured for Rust 1.98.1 and edition 2024. Formatting and linting use rustfmt and Clippy. Run `cargo test --workspace` to check the implemented crates and their documentation examples.

## License

MIT. See [LICENSE.md](LICENSE.md).
