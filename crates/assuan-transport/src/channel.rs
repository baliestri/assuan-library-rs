use crate::{Stream, TransportError};
use assuan_protocol::{LineBuffer, MAX_LINE_BYTES, ProtocolError, Sensitivity};
use std::fmt;
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  time::{Instant, timeout_at},
};
use zeroize::{Zeroize, Zeroizing};

/// Bounded asynchronous line framing with borrowed, mutable receive storage.
///
/// All three buffers have fixed heap storage and are wiped regardless of the
/// supplied sensitivity. Read-ahead may contain a secret not yet classified by
/// the session. Sensitivity describes the operation; it does not change wiping.
/// No protocol state, retries, or response interpretation are performed here.
pub struct Channel {
  stream: Option<Stream>,
  line: Box<LineBuffer>,
  ahead: Zeroizing<Box<[u8]>>,
  pending: usize,
  write: Zeroizing<Box<[u8]>>,
}

impl Channel {
  /// Owns a stream and allocates three fixed buffers before receiving any data.
  #[must_use]
  pub fn new(stream: Stream) -> Self {
    return Self {
      stream: Some(stream),
      line: Box::new(LineBuffer::new()),
      ahead: Zeroizing::new(vec![0; MAX_LINE_BYTES].into_boxed_slice()),
      pending: 0,
      write: Zeroizing::new(vec![0; MAX_LINE_BYTES].into_boxed_slice()),
    };
  }

  /// Reports whether bytes for a subsequent line are already buffered.
  ///
  /// Does not poll the stream or classify those bytes. A session owner can use
  /// this to reject unsolicited read-ahead before beginning another operation.
  #[must_use]
  pub fn has_buffered_input(&self) -> bool {
    return self.pending != 0;
  }

  /// Borrows the last complete line, including any in-place modifications.
  ///
  /// Returns None before completion or after a read starts, an error, or close.
  /// The borrow prevents mutable channel operations while the slice is live.
  #[must_use]
  pub fn received_line(&self) -> Option<&[u8]> {
    return self.line.line();
  }

  /// Reads one LF-terminated line, removing LF and an optional preceding CR.
  ///
  /// The returned slice borrows separate line storage and permits in-place
  /// decoding. Its contents are wiped before the next read. Additional lines
  /// already received remain buffered without another stream read.
  ///
  /// # Errors
  /// Returns framing, I/O, closed-channel, or deadline errors. EOF while waiting
  /// for a line is always an error, including EOF at a line boundary.
  /// Error or cancellation wipes all buffers. Bytes may already have been
  /// consumed: the session owner must invalidate the session before reuse.
  ///
  /// # Panics
  /// Requires a Tokio runtime with time enabled.
  pub async fn read_line(
    &mut self,
    deadline: Instant,
    _sensitivity: Sensitivity,
  ) -> Result<&mut [u8], TransportError> {
    {
      let mut operation = Operation {
        channel: self,
        complete: false,
      };
      operation.channel.line.clear();
      if deadline <= Instant::now() {
        return Err(TransportError::Timeout);
      }
      timeout_at(deadline, operation.channel.receive())
        .await
        .map_err(|_| return TransportError::Timeout)??;
      operation.complete = true;
    }
    return self.line.line_mut().ok_or(TransportError::Closed);
  }

  async fn receive(&mut self) -> Result<(), TransportError> {
    if self.stream.is_none() {
      return Err(TransportError::Closed);
    }
    loop {
      if self.pending != 0 {
        let consumed = self.line.feed(&self.ahead[..self.pending])?;
        self.ahead.copy_within(consumed..self.pending, 0);
        self.pending -= consumed;
        self.ahead[self.pending..].zeroize();
        if self.line.line().is_some() {
          return Ok(());
        }
      }
      self.pending =
        self.stream.as_mut().ok_or(TransportError::Closed)?.read(&mut self.ahead).await?;
      if self.pending == 0 {
        return Err(ProtocolError::UnexpectedEof.into());
      }
    }
  }

  /// Writes one already encoded wire line, including its LF, then flushes.
  ///
  /// The entire operation uses one deadline, including partial writes and flush.
  /// A CR immediately before LF is allowed. This method does not encode payloads
  /// or inspect command names. Caller-owned bytes are never wiped by the channel.
  ///
  /// # Errors
  /// Rejects missing LF, embedded LF/NUL/CR, and lines over the wire limit before
  /// I/O. Returns typed I/O, closed-channel, and timeout errors. Error or
  /// cancellation wipes internal storage; partial delivery is possible and the
  /// owner must invalidate the session before reuse. No retry is performed.
  ///
  /// # Panics
  /// Requires a Tokio runtime with time enabled.
  pub async fn write_line(
    &mut self,
    bytes: &[u8],
    deadline: Instant,
    _sensitivity: Sensitivity,
  ) -> Result<(), TransportError> {
    let mut operation = Operation {
      channel: self,
      complete: false,
    };
    if bytes.len() > MAX_LINE_BYTES {
      return Err(ProtocolError::LineTooLong.into());
    }
    let body = bytes.strip_suffix(b"\n").ok_or(ProtocolError::InvalidLine)?;
    let body = body.strip_suffix(b"\r").unwrap_or(body);
    if body.iter().any(|byte| return matches!(byte, 0 | b'\r' | b'\n')) {
      return Err(ProtocolError::InvalidLine.into());
    }
    if deadline <= Instant::now() {
      return Err(TransportError::Timeout);
    }
    let channel = &mut *operation.channel;
    let stream = channel.stream.as_mut().ok_or(TransportError::Closed)?;
    channel.write[..bytes.len()].copy_from_slice(bytes);
    timeout_at(deadline, async {
      stream.write_all(&channel.write[..bytes.len()]).await?;
      return stream.flush().await;
    })
    .await
    .map_err(|_| return TransportError::Timeout)??;
    channel.write.zeroize();
    operation.complete = true;
    return Ok(());
  }

  /// Drops the stream immediately and wipes every buffer; repeated calls are safe.
  ///
  /// Does not flush or perform protocol shutdown. Pending acknowledgements must
  /// be handled by the session before closing.
  pub fn close(&mut self) {
    self.stream.take();
    self.clear();
  }

  fn clear(&mut self) {
    self.line.clear();
    self.ahead.zeroize();
    self.write.zeroize();
    self.pending = 0;
  }
}

impl fmt::Debug for Channel {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f
      .debug_struct("Channel")
      .field("closed", &self.stream.is_none())
      .finish_non_exhaustive();
  }
}

struct Operation<'a> {
  channel: &'a mut Channel,
  complete: bool,
}

impl Drop for Operation<'_> {
  fn drop(&mut self) {
    if !self.complete {
      self.channel.clear();
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::{
    pin::Pin,
    task::{Context, Poll},
  };
  use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

  fn deadline() -> Instant {
    return Instant::now() + std::time::Duration::from_secs(2);
  }

  fn assert_wiped(channel: &Channel) {
    assert_eq!(channel.pending, 0);
    assert!(channel.line.line().is_none());
    assert!(channel.ahead.iter().all(|byte| return *byte == 0));
    assert!(channel.write.iter().all(|byte| return *byte == 0));
  }

  #[tokio::test]
  async fn read_ahead_is_preserved_then_wiped_before_reuse() {
    let (client, mut peer) = tokio::io::duplex(128);
    peer.write_all(b"OK\nD secret%00\n").await.unwrap();
    let mut channel = Channel::new(Stream::new(client));
    assert_eq!(channel.read_line(deadline(), Sensitivity::Public).await.unwrap(), b"OK");
    assert_eq!(&channel.ahead[..channel.pending], b"D secret%00\n");
    assert!(channel.ahead[channel.pending..].iter().all(|byte| return *byte == 0));
    assert!(!format!("{channel:?}").contains("secret"));
    assert_eq!(channel.read_line(deadline(), Sensitivity::Secret).await.unwrap(), b"D secret%00");
    assert!(channel.ahead.iter().all(|byte| return *byte == 0));
    peer.write_all(b"OK\n").await.unwrap();
    assert_eq!(channel.read_line(deadline(), Sensitivity::Public).await.unwrap(), b"OK");
    channel.close();
    assert_wiped(&channel);
  }

  #[tokio::test]
  async fn cancelling_pending_operations_wipes_allocated_storage() {
    let (client, mut peer) = tokio::io::duplex(8);
    peer.write_all(b"secret").await.unwrap();
    let mut channel = Channel::new(Stream::new(client));
    tokio::select! {
      biased;
      result = channel.read_line(deadline(), Sensitivity::Secret) => panic!("unexpected: {result:?}"),
      () = std::future::ready(()) => {}
    }
    assert_wiped(&channel);
    tokio::select! {
      biased;
      result = channel.write_line(b"D secret-more\n", deadline(), Sensitivity::Secret) => panic!("unexpected: {result:?}"),
      () = std::future::ready(()) => {}
    }
    assert_wiped(&channel);
  }

  #[tokio::test]
  async fn failures_and_successful_writes_wipe_storage() {
    let (client, mut peer) = tokio::io::duplex(2048);
    let mut channel = Channel::new(Stream::new(client));
    channel.write_line(b"D secret\n", deadline(), Sensitivity::Secret).await.unwrap();
    assert!(channel.write.iter().all(|byte| return *byte == 0));
    peer.write_all(&[b's'; 1001]).await.unwrap();
    assert!(channel.read_line(deadline(), Sensitivity::Secret).await.is_err());
    assert_wiped(&channel);
  }

  struct PendingFlush;

  impl AsyncRead for PendingFlush {
    fn poll_read(
      self: Pin<&mut Self>,
      _: &mut Context<'_>,
      _: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
      return Poll::Pending;
    }
  }

  impl AsyncWrite for PendingFlush {
    fn poll_write(
      self: Pin<&mut Self>,
      _: &mut Context<'_>,
      bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
      return Poll::Ready(Ok(bytes.len().min(2)));
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
      return Poll::Pending;
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
      return Poll::Ready(Ok(()));
    }
  }

  #[tokio::test(start_paused = true)]
  async fn flush_shares_write_deadline_and_timeout_wipes_storage() {
    let mut channel = Channel::new(Stream::new(PendingFlush));
    assert!(matches!(
      channel.write_line(b"D secret\n", deadline(), Sensitivity::Secret).await,
      Err(TransportError::Timeout)
    ));
    assert_wiped(&channel);
  }
}
