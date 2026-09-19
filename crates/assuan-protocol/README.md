# assuan-protocol

Runtime-independent building blocks for Assuan. This crate currently provides
fixed-capacity secret storage and explicit borrowed payload classifications.
Wire parsing, encoding, and session state machines are being implemented.

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

## License

MIT. See `LICENSE.md` in this package.
