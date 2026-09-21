use assuan_protocol::{
  ClientState, LimitError, MAX_LINE_BYTES, PayloadRef, SecretBytes, SecretRef, Sensitivity,
  StateError, encode_data_chunk,
};
use tokio::time::Instant;
use zeroize::{Zeroize, Zeroizing};

use crate::{
  ClientError,
  session::{Received, SessionCore},
  transaction::payload,
};

/// Exclusive permission to answer one server inquiry.
///
/// Metadata is copied once into fixed protected storage, independently of the
/// receive buffer. Dropping unfinished work invalidates the session. Forgetting
/// this guard leaves the session in Inquiry, preventing another read or
/// command.
#[derive(Debug)]
#[must_use = "finish or cancel the inquiry before continuing the transaction"]
pub struct Inquiry<'a> {
  core: &'a mut SessionCore,
  metadata: SecretBytes,
  keyword_len: usize,
  sent: usize,
  deadline: Instant,
  completed: bool,
}

impl<'a> Inquiry<'a> {
  pub(crate) fn new(core: &'a mut SessionCore, received: &Received) -> Result<Self, ClientError> {
    let result = (|| {
      let mut metadata = SecretBytes::with_capacity(MAX_LINE_BYTES)?;
      let line = core.channel.received_line().ok_or(ClientError::Incomplete)?;
      metadata.extend_from_slice(&line[received.keyword.clone()])?;
      metadata.extend_from_slice(&line[received.payload.clone()])?;
      let local =
        Instant::now().checked_add(core.inquiry_timeout).ok_or(ClientError::InvalidOptions)?;
      return Ok((metadata, core.deadline.min(local)));
    })();
    let (metadata, deadline) = match result {
      Ok(value) => value,
      Err(error) => {
        core.invalidate();
        return Err(error);
      }
    };
    return Ok(Self {
      core,
      metadata,
      keyword_len: received.keyword.len(),
      sent: 0,
      deadline,
      completed: false,
    });
  }

  pub(crate) fn deadline(&self) -> Instant {
    return self.deadline;
  }

  /// Borrows the validated ASCII keyword until this inquiry is dropped.
  #[must_use]
  pub fn keyword(&self) -> &str {
    // The only constructor copies a keyword validated by parse_server_line.
    return std::str::from_utf8(&self.metadata.expose()[..self.keyword_len]).unwrap_or_default();
  }

  /// Borrows unmodified arguments using the enclosing operation's sensitivity.
  ///
  /// Secret arguments require explicit exposure through `SecretRef`.
  #[must_use]
  pub fn args(&self) -> PayloadRef<'_> {
    return payload(&self.metadata.expose()[self.keyword_len..], self.core.sensitivity);
  }

  /// Sends raw public bytes as one or more escaped D lines.
  ///
  /// Empty input emits an empty D line. Does not send END or extend the
  /// deadline.
  ///
  /// # Errors
  /// Rejects a call that exceeds the remaining decoded-byte budget before
  /// sending any of that call's bytes. Limit, I/O, timeout, or
  /// uncertain-state failures invalidate the session. Cancelling a pending
  /// write prevents reuse.
  ///
  /// # Panics
  /// Requires a Tokio runtime with time enabled.
  pub async fn send_data(&mut self, bytes: &[u8]) -> Result<(), ClientError> {
    return self.send(bytes, Sensitivity::Public).await;
  }

  /// Sends explicitly borrowed secret bytes using fixed protected scratch
  /// space.
  ///
  /// Does not change how incoming responses are classified; use `command_with`
  /// before sending the command to classify the entire response as secret.
  ///
  /// # Errors
  /// Has the same limits, cancellation, and invalidation behavior as
  /// `send_data`. Caller-owned secret storage remains the caller's
  /// responsibility to wipe.
  ///
  /// # Panics
  /// Requires a Tokio runtime with time enabled.
  pub async fn send_secret(&mut self, bytes: SecretRef<'_>) -> Result<(), ClientError> {
    return self.send(bytes.expose(), Sensitivity::Secret).await;
  }

  async fn send(&mut self, bytes: &[u8], sensitivity: Sensitivity) -> Result<(), ClientError> {
    self.check()?;
    let Some(total) = self
      .sent
      .checked_add(bytes.len())
      .filter(|total| return *total <= self.core.max_inquiry_bytes)
    else {
      self.core.invalidate();
      return Err(LimitError::CapacityExceeded.into());
    };
    let mut scratch = Box::new(Zeroizing::new([0; MAX_LINE_BYTES]));
    let mut rest = bytes;
    loop {
      let (consumed, written) = encode_data_chunk(rest, &mut scratch)?;
      self.core.write_at(&scratch[..written], self.deadline, sensitivity).await?;
      scratch.zeroize();
      rest = &rest[consumed..];
      if rest.is_empty() {
        break;
      }
    }
    self.sent = total;
    return Ok(());
  }

  fn check(&mut self) -> Result<(), ClientError> {
    self.core.check()?;
    if self.core.machine.state() != ClientState::Inquiry {
      self.core.invalidate();
      return Err(StateError::NotReady.into());
    }
    return Ok(());
  }

  /// Sends END and resumes the enclosing command or greeting.
  ///
  /// # Errors
  /// I/O, timeout, or cancellation invalidates the session. Successful END does
  /// not finish the enclosing operation; its final response still must be read.
  ///
  /// # Panics
  /// Requires a Tokio runtime with time enabled.
  pub async fn finish(mut self) -> Result<(), ClientError> {
    return self.complete(false).await;
  }

  /// Sends CAN and returns control while awaiting the enclosing final response.
  ///
  /// # Errors
  /// I/O, timeout, or cancellation invalidates the session. A successful CAN is
  /// not final: continue reading the transaction or handshake until OK or ERR.
  ///
  /// # Panics
  /// Requires a Tokio runtime with time enabled.
  pub async fn cancel(mut self) -> Result<(), ClientError> {
    return self.complete(true).await;
  }

  async fn complete(&mut self, cancelled: bool) -> Result<(), ClientError> {
    self.check()?;
    let line = if cancelled {
      b"CAN\n"
    } else {
      b"END\n"
    };
    self.core.write_at(line, self.deadline, self.core.sensitivity).await?;
    self.core.machine.finish_inquiry(cancelled)?;
    self.completed = true;
    return Ok(());
  }
}

impl Drop for Inquiry<'_> {
  fn drop(&mut self) {
    if !self.completed {
      self.core.invalidate();
    }
  }
}
