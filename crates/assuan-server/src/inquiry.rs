use std::fmt;

use assuan_protocol::{
  Command, LimitError, MAX_LINE_BYTES, PayloadRef, ProtocolError, RequestKind, SecretRef,
  Sensitivity, ServerLine, ServerMachine, StateError, decode_data_in_place, encode_response,
};
use assuan_transport::Channel;
use tokio::time::Instant;
use zeroize::Zeroizing;

use crate::{HandlerError, context::IoOperation};

/// How the peer terminated an inquiry. CAN does not decide the command result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InquiryOutcome {
  /// The peer sent END.
  Completed,
  /// The peer sent CAN; the handler must now return its final outcome.
  Cancelled,
}

/// Exclusive streaming access to client inquiry data.
///
/// Finish after next returns None. Dropping unfinished work closes the stream.
/// Forgetting this guard leaves the machine in Inquiry, so the runner cannot
/// send a final or accept another command. Received bytes borrow protected
/// channel storage, preventing another read while a payload is retained.
///
/// A second read cannot invalidate a payload that is still in use:
///
/// ```compile_fail,E0499
/// use assuan_server::{HandlerError, ServerInquiry};
/// async fn overlap(mut inquiry: ServerInquiry<'_>) -> Result<(), HandlerError> {
///   let first = inquiry.next().await?;
///   let second = inquiry.next().await?;
///   drop((first, second));
///   return Ok(());
/// }
/// ```
#[must_use = "consume END or CAN and finish the inquiry"]
pub struct ServerInquiry<'a> {
  channel: &'a mut Channel,
  machine: &'a mut ServerMachine,
  deadline: Instant,
  sensitivity: Sensitivity,
  limit: usize,
  received: usize,
  outcome: Option<InquiryOutcome>,
  finished: bool,
}

impl<'a> ServerInquiry<'a> {
  pub(crate) async fn begin(
    channel: &'a mut Channel,
    machine: &'a mut ServerMachine,
    deadline: Instant,
    limit: usize,
    line: ServerLine<'_>,
    sensitivity: Sensitivity,
  ) -> Result<Self, HandlerError> {
    let mut operation = IoOperation {
      channel,
      machine,
      complete: false,
    };
    let mut output = Box::new(Zeroizing::new([0; MAX_LINE_BYTES]));
    let kind = line.kind();
    let len = encode_response(line, &mut output).map_err(HandlerError::Protocol)?;
    operation.machine.send_response(kind).map_err(HandlerError::State)?;
    operation
      .channel
      .write_line(&output[..len], deadline, sensitivity)
      .await
      .map_err(HandlerError::Transport)?;
    operation.complete = true;
    drop(operation);
    return Ok(Self {
      channel,
      machine,
      deadline,
      sensitivity,
      limit,
      received: 0,
      outcome: None,
      finished: false,
    });
  }

  /// Reads the next decoded data segment, skipping comments and empty lines.
  ///
  /// None means END or CAN was received; call finish to obtain which one.
  /// Repeated calls after None do not read more input. Empty D lines yield an
  /// empty payload. The byte budget counts decoded bytes across all segments.
  /// The returned payload borrows this inquiry and must be released before
  /// another read or finish can reuse its receive storage.
  ///
  /// # Errors
  /// Invalid input, EOF, timeout, or a byte-limit failure closes the channel.
  /// Dropping a polled pending read also invalidates it, even if caught by a
  /// handler.
  ///
  /// # Panics
  /// Requires a Tokio runtime with time enabled.
  pub async fn next(&mut self) -> Result<Option<PayloadRef<'_>>, HandlerError> {
    if self.outcome.is_some() {
      return Ok(None);
    }
    let mut operation = IoOperation {
      channel: self.channel,
      machine: self.machine,
      complete: false,
    };
    loop {
      let line = operation
        .channel
        .read_line(self.deadline, self.sensitivity)
        .await
        .map_err(HandlerError::Transport)?;
      let kind = request_kind(line).map_err(HandlerError::Protocol)?;
      if matches!(kind, RequestKind::End | RequestKind::Can) {
        // Keep Inquiry until finish, even if this guard is deliberately
        // forgotten.
        self.outcome = Some(if kind == RequestKind::End {
          InquiryOutcome::Completed
        } else {
          InquiryOutcome::Cancelled
        });
        operation.complete = true;
        return Ok(None);
      }
      operation.machine.receive_request(kind).map_err(HandlerError::State)?;
      if kind != RequestKind::Data {
        continue;
      }
      let len = decode_data_in_place(&mut line[2..]).map_err(HandlerError::Protocol)?;
      self.received = self
        .received
        .checked_add(len)
        .filter(|total| return *total <= self.limit)
        .ok_or(HandlerError::Limit(LimitError::CapacityExceeded))?;
      operation.complete = true;
      drop(operation);
      let line = self.channel.received_line().ok_or(HandlerError::State(StateError::Unusable))?;
      let data = &line[2..2 + len];
      return Ok(Some(match self.sensitivity {
        Sensitivity::Public => PayloadRef::Public(data),
        Sensitivity::Secret => PayloadRef::Secret(SecretRef::new(data)),
      }));
    }
  }

  /// Releases an inquiry after END or CAN, without reading or draining input.
  ///
  /// # Errors
  /// Returns an error and closes the session if no terminator was received,
  /// or if previous I/O left the machine unusable.
  pub fn finish(mut self) -> Result<InquiryOutcome, HandlerError> {
    let outcome = self.outcome.ok_or(HandlerError::State(StateError::NotReady))?;
    self
      .machine
      .receive_request(match outcome {
        InquiryOutcome::Completed => RequestKind::End,
        InquiryOutcome::Cancelled => RequestKind::Can,
      })
      .map_err(HandlerError::State)?;
    self.finished = true;
    return Ok(outcome);
  }
}

impl Drop for ServerInquiry<'_> {
  fn drop(&mut self) {
    if !self.finished {
      self.machine.invalidate();
      self.channel.close();
    }
  }
}

impl fmt::Debug for ServerInquiry<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f
      .debug_struct("ServerInquiry")
      .field("received", &self.received)
      .field("outcome", &self.outcome)
      .finish_non_exhaustive();
  }
}

pub(crate) fn request_kind(line: &[u8]) -> Result<RequestKind, ProtocolError> {
  if line.iter().any(|b| return matches!(b, 0 | b'\r' | b'\n')) {
    return Err(ProtocolError::InvalidLine);
  }
  if line.is_empty() {
    return Ok(RequestKind::Empty);
  }
  if line.starts_with(b"#") {
    return Ok(RequestKind::Comment);
  }
  if line.starts_with(b"D ") {
    return Ok(RequestKind::Data);
  }
  if line == b"END" {
    return Ok(RequestKind::End);
  }
  if line == b"CAN" {
    return Ok(RequestKind::Can);
  }
  let command = Command::parse(line)?;
  if matches!(command.name(), "END" | "CAN" | "D") {
    return Err(ProtocolError::InvalidLine);
  }
  return Ok(RequestKind::Command);
}
