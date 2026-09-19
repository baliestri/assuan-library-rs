# assuan-transport

Extensible asynchronous byte streams and TCP listeners/connectors for Assuan.
This crate currently implements TCP, Unix sockets, and custom streams. Windows
named pipes, endpoint discovery, and shared protocol framing are planned.

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
