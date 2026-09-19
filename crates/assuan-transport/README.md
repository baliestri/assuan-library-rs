# assuan-transport

Extensible asynchronous byte streams and TCP listeners/connectors for Assuan.
This crate implements TCP, Unix sockets, Windows named pipes, and custom streams.
Agent discovery is supported; shared protocol framing is planned.

## TCP

Run inside a Tokio runtime with I/O and time enabled. Connections have a total
ten-second timeout by default. Binding port zero returns the actual OS-selected
port through `Acceptor::endpoint`.

```rust
use assuan_transport::{Acceptor, ConnectOptions, Endpoint, ListenOptions, Listener};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let endpoint = Endpoint::Tcp("127.0.0.1:0".parse()?);
let mut listener = Listener::bind(&endpoint, &ListenOptions::default()).await?;
let mut client = assuan_transport::connect(listener.endpoint(), &ConnectOptions::default()).await?;
let mut accepted = listener.accept().await?;
assert!(accepted.peer.is_none());
client.write_all(b"\0\xff\n").await?;
let mut bytes = [0; 3];
accepted.stream.read_exact(&mut bytes).await?;
assert_eq!(&bytes, b"\0\xff\n");
# Ok(())
# }
```

TCP supplies no OS-authenticated user identity, including on loopback. The local
access policy in `ListenOptions` applies to local IPC backends, not TCP. Choose
the bind address explicitly and provide any application authentication at the
session layer. This crate does not add TLS or interpret Assuan responses.

## Unix sockets

On Unix, `Endpoint::Unix(path)` connects to a filesystem socket. Binding requires
an existing directory owned by the effective user, with no group or other
permissions (normally mode 0700). A symlink as the immediate parent or endpoint
is rejected, and existing files or sockets are never replaced. The socket is
created inside that private directory, then restricted to mode 0600, without
changing the process-wide umask. The bound endpoint records an absolute path.

Before returning an accepted stream, the listener checks the peer's OS-reported
UID against the effective UID captured at bind time. `Accepted::peer` includes
the Unix UID, GID, and PID when supplied by the platform. Connection attempts do
not require a private parent: callers may connect to external services such as
gpg-agent using their existing endpoint layout.

Call `Listener::cleanup` explicitly to remove an owned socket and close the
listener. It rechecks the parent's owner, permissions, device and inode, and the
socket's owner, type, device and inode. Detected replacements are preserved;
repeated successful cleanup is harmless. Drop closes the listener without
unlinking any pathname. Other processes running as the same user, and privileged
processes, must be trusted: pathname checks cannot prevent their concurrent
mutations. This policy isolates different unprivileged OS users.

## Windows named pipes

`Endpoint::NamedPipe` accepts local paths such as `\\.\pipe\assuan-example`.
Remote names, embedded NUL, empty names, nested separators, and paths longer than
256 UTF-16 code units are rejected. Listeners use byte mode, reject remote
clients, and create noninheritable handles. An existing pipe name is rejected.

Before creating the first instance, the listener queries the process token's
user SID and installs an explicit protected DACL with one allow entry for that
SID. ACL construction failure aborts binding; no default or null DACL is used.
This does not populate `Accepted::peer`: an access policy is not independently
queried peer identity. The native listener keeps one waiting instance in addition
to accepted connections; application session limits must account for it.

A busy-pipe connection retries asynchronously with a short timer under the total
connection deadline. Cancellation does not leave a blocking connection worker.
The adapter's flush is a no-op because writes already reach the kernel buffer;
it does not wait for the peer to consume them. Shutdown closes both directions,
and Drop closes directly without starting the backend's background drain.
Unread buffered data may be discarded. Higher-level protocols must complete
required acknowledgements before closing, and must not assume TCP half-close
semantics. No raw Windows APIs or unsafe code are used in this workspace.

## Agent discovery

`AgentLocator` invokes an explicitly selected `gpgconf` executable without a
shell. Discovery has a five-second deadline and a combined one-MiB output limit.
Unix path bytes are preserved. Windows native socket files are parsed as a port
and 16 binary nonce bytes, with protected storage and redacted diagnostics.
Cygwin socket files are not supported.

```rust,no_run
use assuan_transport::{AgentLocator, ConnectOptions};
use std::path::PathBuf;

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let locator = AgentLocator::new(PathBuf::from("gpgconf"), None);
let resolved = locator.resolve().await?;
let stream = resolved.connect(&ConnectOptions::default()).await?;
// The session layer can now read the greeting from stream.
# drop(stream);
# Ok(())
# }
```

Use `resolved.connect(...)` to include the native Windows nonce handshake.
The generic connector given only `resolved.endpoint()` does not send the nonce.
Discovery does not launch an agent or consume its greeting. Connecting directly
with an explicit endpoint does not require `GnuPG`.

The process fixture tests require the `test-fixtures` feature:
`cargo test -p assuan-transport --features test-fixtures`.

## Custom transports

`Stream::new` accepts any owned `AsyncRead + AsyncWrite + Unpin + Send` stream.
The boxed wrapper normally allocates once per connection, not per byte; it
delegates reads, writes, vectored writes, flush, and shutdown without adding
buffers. `Acceptor` supports custom listeners using a borrowed boxed future,
which allocates once per acceptance operation. Debug output does not inspect
the wrapped stream. I/O errors preserve their causes explicitly, while the
transport error's own Display and Debug omit external error text.

```rust
use assuan_transport::Stream;
let (left, right) = tokio::io::duplex(16);
let stream = Stream::new(left);
assert_eq!(format!("{stream:?}"), "Stream { .. }");
drop(right);
```

The wrapper does not wipe caller buffers or OS buffers. Session ownership,
transaction cancellation, framing, and sensitive-data handling belong to higher
layers. Dropping a stream drops its transport; no background drain is spawned.

## License

MIT. See `LICENSE.md` in this package.
