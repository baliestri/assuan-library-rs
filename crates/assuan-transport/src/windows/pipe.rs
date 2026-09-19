use crate::{Accepted, Stream, TransportError};
use interprocess::{
  ConnectWaitMode,
  os::windows::named_pipe::{
    PipeListenerOptions, pipe_mode,
    tokio::{DuplexPipeStream, PipeListener as NativeListener},
  },
};
use std::{
  io,
  os::windows::ffi::OsStrExt,
  path::Path,
  pin::Pin,
  task::{Context, Poll},
  time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

type NativeStream = DuplexPipeStream<pipe_mode::Bytes>;

pub(crate) struct PipeListener {
  inner: NativeListener<pipe_mode::Bytes, pipe_mode::Bytes>,
}

impl PipeListener {
  pub(crate) fn bind(path: &Path) -> Result<Self, TransportError> {
    validate_path(path)?;
    let inner = PipeListenerOptions::new()
      .path(path)
      .accept_remote(false)
      .inheritable(false)
      .security_descriptor(Some(super::security::current_user_descriptor()?))
      .create_tokio_duplex::<pipe_mode::Bytes>()?;
    return Ok(Self {
      inner,
    });
  }

  pub(crate) async fn accept(&self) -> Result<Accepted, TransportError> {
    let stream = self.inner.accept().await?;
    return Ok(Accepted {
      stream: Stream::new(PipeStream {
        inner: Some(stream),
      }),
      peer: None,
    });
  }
}

pub(crate) async fn connect(path: &Path) -> Result<Stream, TransportError> {
  validate_path(path)?;
  loop {
    // A zero wait avoids interprocess's blocking connection worker. The outer
    // connect deadline owns cancellation; retries yield to the runtime timer.
    match NativeStream::connect_by_path_with_wait_mode(
      path,
      ConnectWaitMode::Timeout(Duration::ZERO),
    )
    .await
    {
      Ok(stream) => {
        return Ok(Stream::new(PipeStream {
          inner: Some(stream),
        }));
      }
      Err(error)
        if error.kind() == io::ErrorKind::TimedOut || error.raw_os_error() == Some(231) =>
      {
        tokio::time::sleep(Duration::from_millis(10)).await;
      }
      Err(error) => return Err(error.into()),
    }
  }
}

fn validate_path(path: &Path) -> Result<(), TransportError> {
  let units: Vec<u16> = path.as_os_str().encode_wide().collect();
  let prefix: Vec<u16> = r"\\.\pipe\".encode_utf16().collect();
  if !units.starts_with(&prefix)
    || units.len() <= prefix.len()
    || units.len() > 256
    || units[prefix.len()..].iter().any(|unit| return matches!(*unit, 0 | 47 | 92))
  {
    return Err(TransportError::InvalidEndpoint);
  }
  return Ok(());
}

// The native backend's flush waits for the peer to read and its Drop can spawn
// a background drainer. Writes already enter the kernel buffer: expose a no-op
// flush like Tokio's byte pipes, and close directly on shutdown/drop instead.
struct PipeStream {
  inner: Option<NativeStream>,
}

impl Drop for PipeStream {
  fn drop(&mut self) {
    if let Some(stream) = self.inner.take() {
      stream.evade_limbo();
    }
  }
}

impl AsyncRead for PipeStream {
  fn poll_read(
    self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buffer: &mut ReadBuf<'_>,
  ) -> Poll<io::Result<()>> {
    let Some(stream) = self.get_mut().inner.as_mut() else {
      return Poll::Ready(Ok(()));
    };
    return Pin::new(stream).poll_read(cx, buffer);
  }
}

impl AsyncWrite for PipeStream {
  fn poll_write(
    self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    bytes: &[u8],
  ) -> Poll<io::Result<usize>> {
    let Some(stream) = self.get_mut().inner.as_mut() else {
      return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
    };
    return Pin::new(stream).poll_write(cx, bytes);
  }
  fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
    if self.inner.is_none() {
      return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
    }
    return Poll::Ready(Ok(()));
  }
  fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
    if let Some(stream) = self.get_mut().inner.take() {
      stream.evade_limbo();
    }
    return Poll::Ready(Ok(()));
  }
}
