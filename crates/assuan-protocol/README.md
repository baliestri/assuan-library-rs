# assuan-protocol

Runtime-independent building blocks for Assuan. This crate currently provides
fixed-capacity secret storage, borrowed command and response parsing, and
in-place percent decoding. Framing, encoding, and session state machines are
being implemented.

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

## License

MIT. See `LICENSE.md` in this package.
