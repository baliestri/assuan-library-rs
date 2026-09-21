//! Runs a custom in-memory byte transport without external services.

use std::{
  error::Error,
  io,
  pin::Pin,
  task::{Context, Poll},
};

use assuan_library::{
  Accepted, Acceptor, Client, ClientOptions, CollectLimits, Command, CommandContext, HandlerFuture,
  IoFuture, Server, ServerOptions, Stream, TransportError, handler,
};
use tokio::{
  io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf},
  sync::{mpsc, oneshot},
};

// Application-owned adapter. A real transport must also preserve wakeups,
// partial reads/writes and cancellation semantics.
struct ByteAdapter(DuplexStream);

impl AsyncRead for ByteAdapter {
  fn poll_read(
    self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buffer: &mut ReadBuf<'_>,
  ) -> Poll<io::Result<()>> {
    return Pin::new(&mut self.get_mut().0).poll_read(cx, buffer);
  }
}

impl AsyncWrite for ByteAdapter {
  fn poll_write(
    self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    bytes: &[u8],
  ) -> Poll<io::Result<usize>> {
    return Pin::new(&mut self.get_mut().0).poll_write(cx, bytes);
  }

  fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    return Pin::new(&mut self.get_mut().0).poll_flush(cx);
  }

  fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    return Pin::new(&mut self.get_mut().0).poll_shutdown(cx);
  }
}

struct MemoryAcceptor(mpsc::Receiver<Accepted>);

impl Acceptor for MemoryAcceptor {
  // recv is cancellation-safe: a cancelled accept does not consume a stream.
  // endpoint() intentionally retains its default None.
  fn accept(&mut self) -> IoFuture<'_, Accepted> {
    return Box::pin(async move {
      return self.0.recv().await.ok_or(TransportError::Closed);
    });
  }
}

fn echo<'a>(command: Command<'a>, mut context: CommandContext<'a>) -> HandlerFuture<'a> {
  return Box::pin(async move {
    return context.send_data(command.args()).await;
  });
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
  let (incoming, receiver) = mpsc::channel(1);
  let acceptor = MemoryAcceptor(receiver);
  assert!(acceptor.endpoint().is_none());
  let mut server = Server::new(|| (), ServerOptions::default().with_max_sessions(1)?);
  server.register(handler("ECHO", "Returns public arguments", echo)?)?;
  let (stop, stopped) = oneshot::channel();
  let serving = tokio::spawn(server.serve(acceptor, async {
    let _ = stopped.await;
  }));
  let (client_io, server_io) = tokio::io::duplex(1024);
  incoming
    .send(Accepted {
      stream: Stream::new(ByteAdapter(server_io)),
      peer: None,
    })
    .await?;
  let mut client =
    Client::from_stream(Stream::new(ByteAdapter(client_io)), ClientOptions::default()).await?;
  let response = client.collect(Command::new("ECHO", b"custom")?, CollectLimits::default()).await?;
  assert_eq!(response.data(), b"custom");
  client.collect(Command::new("BYE", b"")?, CollectLimits::default()).await?;
  drop(client);
  let _ = stop.send(());
  serving.await??;
  println!("Custom transport command and shutdown completed");
  return Ok(());
}
