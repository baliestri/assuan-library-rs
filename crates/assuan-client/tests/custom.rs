//! Consumer-defined byte streams work without standard endpoint metadata.
use std::{
  io,
  pin::Pin,
  task::{Context, Poll},
};

use assuan_client::{Client, ClientOptions, Event};
use assuan_protocol::Command;
use assuan_transport::Stream;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf};

// Deliberately does not implement Debug or use a standard Endpoint.
struct CustomIo(DuplexStream);

impl AsyncRead for CustomIo {
  fn poll_read(
    self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buffer: &mut ReadBuf<'_>,
  ) -> Poll<io::Result<()>> {
    return Pin::new(&mut self.get_mut().0).poll_read(cx, buffer);
  }
}

impl AsyncWrite for CustomIo {
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

#[tokio::test]
async fn custom_stream_preserves_command_bytes_and_completes_a_transaction() {
  let (io, mut peer) = tokio::io::duplex(4);
  let remote = tokio::spawn(async move {
    peer.write_all(b"OK custom\n").await.unwrap();
    let mut bytes = [0; 14];
    peer.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"CUSTOM a%20b\xff\n");
    peer.write_all(b"OK\n").await.unwrap();
  });
  let mut client =
    Client::from_stream(Stream::new(CustomIo(io)), ClientOptions::default()).await.unwrap();
  let mut tx = client.command(Command::new("CUSTOM", b"a%20b\xff").unwrap()).await.unwrap();
  assert!(matches!(
    tx.next().await.unwrap(),
    Some(Event::Finished {
      code: None,
      ..
    })
  ));
  tx.finish().unwrap();
  assert!(client.is_usable());
  remote.await.unwrap();
}

#[tokio::test]
async fn tcp_connector_consumes_greeting_before_commands() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let endpoint = assuan_transport::Endpoint::Tcp(listener.local_addr().unwrap());
  let remote = tokio::spawn(async move {
    let (mut peer, _) = listener.accept().await.unwrap();
    peer.write_all(b"OK\n").await.unwrap();
    let mut bytes = [0; 4];
    peer.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"NOP\n");
    peer.write_all(b"OK\n").await.unwrap();
  });
  let mut client = Client::connect(&endpoint, ClientOptions::default()).await.unwrap();
  let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  tx.next().await.unwrap();
  tx.finish().unwrap();
  remote.await.unwrap();
}
