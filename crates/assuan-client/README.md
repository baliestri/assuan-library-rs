# assuan-client

Asynchronous Assuan clients with one exclusive streaming transaction per session.
Use separate connections for concurrent commands. Standard transport connections
use `Client::connect`; `Client::from_stream` accepts a connected custom `Stream`
without endpoint metadata or agent discovery.

## Transactions

```rust
use assuan_client::{Client, ClientOptions, Event, PayloadRef};
use assuan_protocol::Command;
use assuan_transport::Stream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let (io, mut peer) = tokio::io::duplex(128);
let remote = tokio::spawn(async move {
  peer.write_all(b"OK ready\n").await.unwrap();
  let mut command = [0; 4];
  peer.read_exact(&mut command).await.unwrap();
  assert_eq!(&command, b"NOP\n");
  peer.write_all(b"D hello%0A\nOK\n").await.unwrap();
});
let mut client = Client::from_stream(Stream::new(io), ClientOptions::default()).await?;
let mut transaction = client.command(Command::new("NOP", b"")?).await?;
while let Some(event) = transaction.next().await? {
  if let Event::Data(PayloadRef::Public(bytes)) = event {
    assert_eq!(bytes, b"hello\n");
  }
}
transaction.finish()?;
assert!(client.is_usable());
remote.await?;
# Ok(())
# }
```

Command arguments are already in wire representation. The client validates the
wire length and encodes into a fixed protected heap buffer before sending. Data
responses are decoded in place; status arguments and diagnostic text retain their
wire bytes. Events borrow the receive storage without payload allocation. Empty
lines are skipped, and END does not finish a command: only OK or ERR does.

`finish` performs no I/O. It requires a final response already received and returns
`ClientError::Remote` with the full `u32` code for ERR. A normal remote error leaves
the session reusable. Dropping a transaction after its final response is also safe.

## Borrowing and cancellation

The client is borrowed exclusively until the transaction ends:

```compile_fail
use assuan_client::Client;
use assuan_protocol::Command;
async fn overlap(client: &mut Client) {
  let mut first = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  let _second = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  first.next().await.unwrap();
}
```

An event cannot remain in use across the next mutable transaction operation:

```compile_fail
use assuan_client::Transaction;
async fn retain_event(transaction: &mut Transaction<'_>) {
  let event = transaction.next().await.unwrap();
  transaction.next().await.unwrap();
  println!("{event:?}");
}
```

Dropping unfinished work, premature `finish`, EOF without a final response, and
malformed responses invalidate the session and close its stream. Forgetting an
unfinished transaction does not reset its phase; another command fails before
writing. No command is automatically retried.

Cancelling a pending command write or response read leaves an uncertainty marker
in the client even when the future disappears. `is_usable()` becomes false; the
next mutable operation or transaction drop closes the stream. Dropping an unpolled
future performs no I/O and does not change session state. During construction,
cancellation drops the owned stream immediately.

Read-ahead responses are checked in the idle state before a new command is sent;
a buffered duplicate final response invalidates the connection. This check covers
bytes already read by the channel, not bytes still in transit. Assuan has no
transaction identifiers that could reliably distinguish a late unsolicited
response from a response to a subsequent command.

## Timeouts and sensitive data

`ClientOptions` defaults to ten seconds for connection, 120 seconds for greeting,
and 300 seconds per command. Zero or unrepresentable durations are rejected before
I/O. The command deadline includes writing, receiving all events, and time spent
by the caller between events; data does not restart it. `is_usable` reports local
readiness, not remote liveness.

Current commands expose public `PayloadRef` values. All internal line, read-ahead,
and outgoing buffers are nevertheless wiped; public Debug output omits payloads,
keywords, and diagnostic text. Borrowed caller arguments and OS buffers are not
wiped by the client. Explicit secret classification and interactive inquiries are
not yet implemented. Receiving an inquiry currently closes the connection with
`UnsupportedInquiry`.

## License

MIT. See `LICENSE.md` in this package.