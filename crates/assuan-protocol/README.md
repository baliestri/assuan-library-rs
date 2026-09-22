# assuan-protocol

Runtime-independent building blocks for Assuan. This crate currently provides
fixed-capacity secret storage, borrowed command and response parsing, and
in-place percent decoding, incremental framing, bounded encoding, and pure
client and server session state machines.

Disable default features for `no_std` with `alloc`. This crate requires a heap
allocator, but has no I/O or asynchronous runtime dependency.

## Secret storage

`SecretBytes` allocates and zero-initializes its entire capacity before accepting
secret bytes. Appends never reallocate; an over-capacity append fails without
changing the buffer. `clear` wipes the entire allocation while retaining it for
reuse. Normal drop wipes the allocation through `zeroize` before deallocation.

```rust
use assuan_protocol::{PayloadRef, SecretBytes, SecretRef};

let mut secret = SecretBytes::with_capacity(128)?;
secret.extend_from_slice(b"example")?;
let payload = PayloadRef::Secret(SecretRef::new(secret.expose()));
assert!(!format!("{payload:?}").contains("example"));
secret.clear();
# Ok::<(), assuan_protocol::LimitError>(())
```

Choose capacity from a trusted resource limit. `SecretBytes`, `SecretRef`, and
`PayloadRef` do not implement `Clone` or `Copy`. Their debug output reports only
metadata, even for public payloads. Reading protected bytes requires explicit
exposure. Error messages never contain payload bytes.

Borrowed views do not wipe their owner's storage. The caller must protect and
clean up source buffers and any copies made after exposure. Zeroization does
not cover primitive values moved on the stack, OS buffers, swap, or crash dumps.
Leaking a value with `mem::forget` or terminating the process without running
destructors prevents cleanup. The heap allocation stays at the same address when
the wrapper moves; no memory locking is performed.

## Wire parsing

`Command::parse` and `parse_server_line` borrow their input without allocating.
Pass a line without its LF or CRLF terminator. Line-length checks belong to the
framing layer. Command arguments retain their original spaces, binary bytes,
and escapes; handlers decide how to interpret them.

```rust
use assuan_protocol::{Command, ServerLine, parse_server_line};

let command = Command::parse(b"GETINFO version")?;
assert_eq!(command.name(), "GETINFO");
assert_eq!(command.args(), b"version");
assert!(matches!(parse_server_line(b"OK done")?, ServerLine::Ok(b"done")));
# Ok::<(), assuan_protocol::ProtocolError>(())
```

Data responses remain percent-encoded. `decode_data_in_place` validates every
escape before modifying input, accepts both hexadecimal cases, and returns the
decoded prefix length. Bytes beyond that prefix may still contain old data;
protected buffer owners must wipe the entire allocation. Parser diagnostics
never include input contents.

The wire grammar follows the Assuan manual's
[client requests](https://www.gnupg.org/documentation/manuals/assuan/Client-requests.html)
and [server responses](https://www.gnupg.org/documentation/manuals/assuan/Server-responses.html).

## Framing and encoding

`LineBuffer` consumes through one LF at a time. Use the consumed count to retain
any following bytes for the next line. Both LF and CRLF count toward the
1,000-byte wire limit. A complete line remains borrowed until `clear`; EOF with
an unfinished line is an error. Clear, framing failure, and normal drop wipe
its fixed storage. Moving stack storage may leave copies, so transports must
also protect their read-ahead and scratch buffers.

```rust
use assuan_protocol::{LineBuffer, MAX_LINE_BYTES, encode_data_chunk};

let mut output = [0; MAX_LINE_BYTES];
let (consumed, written) = encode_data_chunk(b"first\nsecond", &mut output)?;
assert_eq!(consumed, 12);
let mut frame = LineBuffer::new();
assert_eq!(frame.feed(&output[..written])?, written);
assert_eq!(frame.line(), Some(b"D first%0Asecond".as_slice()));
frame.clear();
# Ok::<(), assuan_protocol::ProtocolError>(())
```

`encode_command` and `encode_response` validate the complete line before writing;
errors preserve the output. `encode_data_chunk` takes raw bytes and emits as much
as fits, without splitting an escape. Repeat with the unconsumed suffix for
larger payloads. `ServerLine::Data`, in contrast, holds wire-format bytes and
is not escaped again. Output buffers belong to the caller and require cleanup
when carrying sensitive data.

## Session state machines

`ClientMachine` and `ServerMachine` validate protocol sequencing without I/O or
allocation. Both begin in Greeting. Comments, status, and inquiries may precede
the initial OK. An inquiry remembers whether to resume the greeting or a command.

```rust
use assuan_protocol::{ClientMachine, ClientState, LineKind};

let mut client = ClientMachine::new();
client.receive(LineKind::Ok)?;
client.begin_command()?;
client.receive(LineKind::Inquire)?;
client.finish_inquiry(true)?; // CAN was successfully sent.
assert_eq!(client.state(), ClientState::AwaitFinal);
client.receive(LineKind::Err)?;
assert_eq!(client.state(), ClientState::Ready);
# Ok::<(), assuan_protocol::StateError>(())
```

A command's ERR permits reuse; a greeting's ERR invalidates the connection.
CAN requires a final response and must not be confused with the reserved
application command CANCEL. Partial server END does not finish a command.
Unexpected protocol events invalidate the machine. Local client operations
requested in the wrong phase fail without discarding an otherwise valid session.

The integration layer must validate wire syntax before classifying events and
invalidate after failed, partial, or cancelled I/O. Server response transitions
are checked before sending; a failed send must invalidate even if the transition
reached Ready. These machines do not close transports or implement timeout logic.

## Platform, feature and security boundaries

The default `std` feature can be disabled for `no_std` with `alloc`. Protocol logic is platform-independent and performs no I/O. Borrowed commands and server lines cannot outlive their input; the caller owns decoding and output buffers. Higher layers enforce deadlines and transport closure after uncertain I/O.

See the [security guide](https://github.com/baliestri/assuan-library-rs/blob/develop/docs/security.md) for storage, cancellation and transport guarantees, and the [package guide](https://github.com/baliestri/assuan-library-rs/blob/develop/docs/publishing.md) for local verification and release prerequisites.

## License

MIT. See `LICENSE.md` in this package.
