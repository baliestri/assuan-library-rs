use std::{
  fmt, io,
  pin::Pin,
  task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// An asynchronous, movable, sendable duplex byte stream.
///
/// Implemented automatically for matching Tokio-compatible stream types.
/// Custom transports must obey the `AsyncRead` and `AsyncWrite` contracts,
/// including wakeups, short writes, flush, and shutdown behavior.
pub trait IoStream: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send + ?Sized> IoStream for T {}

/// A type-erased stream with redacted diagnostics and safe I/O delegation.
///
/// Wrapping a nonzero-sized stream requires one heap allocation, not one per
/// byte or I/O call. This wrapper adds no buffering, framing, cleanup of caller
/// buffers, or background tasks. Dropping it drops the underlying stream.
pub struct Stream {
  inner: Box<dyn IoStream>,
}

impl Stream {
  /// Takes ownership of any compatible stream, without reading or writing it.
  #[must_use]
  pub fn new<T: IoStream + 'static>(stream: T) -> Self {
    return Self {
      inner: Box::new(stream),
    };
  }
}

impl fmt::Debug for Stream {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f.debug_struct("Stream").finish_non_exhaustive();
  }
}

impl AsyncRead for Stream {
  fn poll_read(
    self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buffer: &mut ReadBuf<'_>,
  ) -> Poll<io::Result<()>> {
    return Pin::new(&mut *self.get_mut().inner).poll_read(cx, buffer);
  }
}

impl AsyncWrite for Stream {
  fn poll_write(
    self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    bytes: &[u8],
  ) -> Poll<io::Result<usize>> {
    return Pin::new(&mut *self.get_mut().inner).poll_write(cx, bytes);
  }

  fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    return Pin::new(&mut *self.get_mut().inner).poll_flush(cx);
  }

  fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    return Pin::new(&mut *self.get_mut().inner).poll_shutdown(cx);
  }

  fn poll_write_vectored(
    self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buffers: &[io::IoSlice<'_>],
  ) -> Poll<io::Result<usize>> {
    return Pin::new(&mut *self.get_mut().inner).poll_write_vectored(cx, buffers);
  }

  fn is_write_vectored(&self) -> bool {
    return self.inner.is_write_vectored();
  }
}
