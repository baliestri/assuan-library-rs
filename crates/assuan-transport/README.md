# assuan-transport

Extensible asynchronous byte streams and TCP listeners/connectors for Assuan.
This crate currently implements TCP and custom streams. Unix sockets, Windows
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
