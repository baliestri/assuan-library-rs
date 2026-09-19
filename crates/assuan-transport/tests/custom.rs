//! User-provided transports and listeners delegate I/O without unsafe code.

use assuan_transport::{Accepted, Acceptor, Endpoint, IoFuture, Stream, TransportError};
use std::{
  io,
  pin::Pin,
  sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
  },
  task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};

struct AuditedStream(Arc<AtomicUsize>);

impl AsyncRead for AuditedStream {
  fn poll_read(
    self: Pin<&mut Self>,
    _: &mut Context<'_>,
    _: &mut ReadBuf<'_>,
  ) -> Poll<io::Result<()>> {
    return Poll::Ready(Ok(()));
  }
}

impl AsyncWrite for AuditedStream {
  fn poll_write(
    self: Pin<&mut Self>,
    _: &mut Context<'_>,
    bytes: &[u8],
  ) -> Poll<io::Result<usize>> {
    return Poll::Ready(Ok(bytes.len()));
  }
  fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
    self.0.fetch_or(1, Ordering::SeqCst);
    return Poll::Ready(Ok(()));
  }
  fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
    self.0.fetch_or(2, Ordering::SeqCst);
    return Poll::Ready(Ok(()));
  }
  fn is_write_vectored(&self) -> bool {
    return true;
  }
  fn poll_write_vectored(
    self: Pin<&mut Self>,
    _: &mut Context<'_>,
    buffers: &[io::IoSlice<'_>],
  ) -> Poll<io::Result<usize>> {
    self.0.fetch_or(4, Ordering::SeqCst);
    return Poll::Ready(Ok(buffers.iter().map(|part| return part.len()).sum()));
  }
}

#[tokio::test]
async fn vectored_write_flush_and_shutdown_reach_the_custom_stream() {
  let calls = Arc::new(AtomicUsize::new(0));
  let mut stream = Stream::new(AuditedStream(Arc::clone(&calls)));
  assert!(stream.is_write_vectored());
  assert_eq!(
    stream.write_vectored(&[io::IoSlice::new(b"ab"), io::IoSlice::new(b"cd")]).await.unwrap(),
    4
  );
  stream.flush().await.unwrap();
  stream.shutdown().await.unwrap();
  assert_eq!(calls.load(Ordering::SeqCst), 7);
}

struct CustomAcceptor {
  stream: Option<Stream>,
  endpoint: Endpoint,
}

impl Acceptor for CustomAcceptor {
  fn accept(&mut self) -> IoFuture<'_, Accepted> {
    return Box::pin(async {
      return Ok(Accepted {
        stream: self.stream.take().ok_or(TransportError::InvalidEndpoint)?,
        peer: None,
      });
    });
  }
  fn endpoint(&self) -> &Endpoint {
    return &self.endpoint;
  }
}

#[tokio::test]
async fn acceptor_is_object_safe_and_supports_custom_streams() {
  let (left, _right) = tokio::io::duplex(4);
  let mut acceptor: Box<dyn Acceptor> = Box::new(CustomAcceptor {
    stream: Some(Stream::new(left)),
    endpoint: Endpoint::Tcp("127.0.0.1:1".parse().unwrap()),
  });
  assert!(acceptor.accept().await.unwrap().peer.is_none());
  assert!(acceptor.accept().await.is_err());
}
