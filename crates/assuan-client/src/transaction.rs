use std::fmt;

use assuan_protocol::{ClientState, LineKind, PayloadRef, SecretRef, Sensitivity};

use crate::{ClientError, session::SessionCore};

/// One response borrowing the session's receive buffer until the next mutable
/// use.
///
/// Data is percent-decoded in place; other fields retain their wire bytes.
/// Debug omits all received strings, including status keywords.
#[non_exhaustive]
pub enum Event<'a> {
  /// Exclusive request for client-provided data; finish or cancel before
  /// reading again.
  Inquire(crate::Inquiry<'a>),
  /// A decoded data segment; does not complete the transaction.
  Data(PayloadRef<'a>),
  /// Informational status with unmodified arguments.
  Status {
    /// Validated ASCII status keyword.
    keyword: &'a str,
    /// Borrowed wire arguments.
    args: PayloadRef<'a>,
  },
  /// Comment bytes after `#`, including any leading space.
  Comment(PayloadRef<'a>),
  /// Partial end of data; a final OK or ERR is still required.
  End,
  /// Final response; call finish to obtain a typed remote error when present.
  Finished {
    /// None for OK, or the full unsigned ERR code.
    code: Option<u32>,
    /// Borrowed diagnostic text, excluded from error messages.
    text: PayloadRef<'a>,
  },
}

impl fmt::Debug for Event<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f.write_str(match self {
      Self::Inquire(_) => "Inquire { .. }",
      Self::Data(_) => "Data { .. }",
      Self::Status {
        ..
      } => "Status { .. }",
      Self::Comment(_) => "Comment { .. }",
      Self::End => "End",
      Self::Finished {
        ..
      } => "Finished { .. }",
    });
  }
}

/// An exclusive command transaction; dropping unfinished work closes its
/// stream.
///
/// Forgetting this guard does not reset the client's command phase. Receiving a
/// final response makes dropping the guard safe; finish performs no network
/// I/O.
#[derive(Debug)]
#[must_use = "consume the final response and finish the transaction"]
pub struct Transaction<'a> {
  pub(crate) core: &'a mut SessionCore,
  pub(crate) completion: Completion,
}

#[derive(Debug)]
pub(crate) enum Completion {
  Pending,
  Success,
  Remote(u32),
}

impl Transaction<'_> {
  /// Reads the next event under the original total command deadline.
  ///
  /// Empty lines are skipped. After a final response this returns None without
  /// another read. The returned event borrows this transaction, so it must be
  /// released before the next mutable operation.
  ///
  /// # Errors
  /// Malformed responses, EOF, timeout, or forgotten inquiries invalidate the
  /// session. Cancelling a pending read marks I/O uncertain even if this guard
  /// remains alive; its next operation fails without further I/O.
  ///
  /// # Panics
  /// Requires a Tokio runtime with time enabled.
  pub async fn next(&mut self) -> Result<Option<Event<'_>>, ClientError> {
    return next_event(self.core, &mut self.completion).await;
  }

  /// Consumes an already completed transaction, without draining or other I/O.
  ///
  /// # Errors
  /// Returns the full remote ERR code, or Incomplete and closes the stream if
  /// no final response was received. A normal remote error preserves session
  /// reuse.
  pub fn finish(self) -> Result<(), ClientError> {
    self.core.check()?;
    return match self.completion {
      Completion::Remote(code) => {
        Err(ClientError::Remote {
          code,
        })
      }
      Completion::Success => Ok(()),
      Completion::Pending => Err(ClientError::Incomplete),
    };
  }
}

impl Drop for Transaction<'_> {
  fn drop(&mut self) {
    if self.core.io_uncertain || self.core.machine.state() != ClientState::Ready {
      self.core.invalidate();
    }
  }
}

pub(crate) fn payload(bytes: &[u8], sensitivity: Sensitivity) -> PayloadRef<'_> {
  return match sensitivity {
    Sensitivity::Public => PayloadRef::Public(bytes),
    Sensitivity::Secret => PayloadRef::Secret(SecretRef::new(bytes)),
  };
}

pub(crate) async fn next_event<'a>(
  core: &'a mut SessionCore,
  completion: &mut Completion,
) -> Result<Option<Event<'a>>, ClientError> {
  core.check()?;
  if core.machine.state() == ClientState::Inquiry {
    core.invalidate();
    return Err(assuan_protocol::StateError::NotReady.into());
  }
  if !matches!(*completion, Completion::Pending) {
    return Ok(None);
  }
  let received = loop {
    let received = core.read().await?;
    if received.kind != LineKind::Empty {
      break received;
    }
  };
  if matches!(received.kind, LineKind::Ok | LineKind::Err) {
    *completion = received.code.map_or(Completion::Success, Completion::Remote);
  }
  if received.kind == LineKind::Inquire {
    return Ok(Some(Event::Inquire(crate::Inquiry::new(core, &received)?)));
  }
  let line = core.channel.received_line().ok_or(ClientError::Incomplete)?;
  let payload = payload(&line[received.payload], core.sensitivity);
  let event = match received.kind {
    LineKind::Data => Event::Data(payload),
    LineKind::Comment => Event::Comment(payload),
    LineKind::End => Event::End,
    LineKind::Status => {
      Event::Status {
        keyword: std::str::from_utf8(&line[received.keyword])
          .map_err(|_| return ClientError::Incomplete)?,
        args: payload,
      }
    }
    LineKind::Ok | LineKind::Err => {
      Event::Finished {
        code: received.code,
        text: payload,
      }
    }
    LineKind::Empty | LineKind::Inquire => return Err(ClientError::Incomplete),
  };
  return Ok(Some(event));
}
