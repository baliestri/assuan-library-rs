use std::fmt;

use assuan_protocol::{LineKind, MAX_LINE_BYTES, Sensitivity, ServerMachine, encode_data_chunk};
use assuan_transport::{Channel, PeerIdentity};
use tokio::time::Instant;
use zeroize::{Zeroize, Zeroizing};

use crate::{HandlerError, ServerInquiry, ServerOptions};

/// Exclusive access to one invocation's channel and session-local state.
///
/// Created by the session runner, never by handlers. The lifetime prevents
/// retention beyond the invocation. Final OK/ERR responses are reserved for
/// the runner. Debug never inspects state or payloads.
pub struct CommandContext<'io, S: Send + 'static = ()> {
  pub(crate) channel: &'io mut Channel,
  pub(crate) machine: &'io mut ServerMachine,
  pub(crate) state: &'io mut S,
  pub(crate) deadline: Instant,
  pub(crate) options: &'io ServerOptions,
  pub(crate) peer: Option<&'io PeerIdentity>,
  pub(crate) authenticated: bool,
}

impl<S: Send + 'static> CommandContext<'_, S> {
  /// Borrows transport-supplied OS identity, absent for TCP.
  #[must_use]
  pub fn peer(&self) -> Option<&PeerIdentity> {
    return self.peer;
  }

  /// Reports whether the session's authentication hook has already succeeded.
  ///
  /// This stays true across RESET; it does not imply that `DefaultHooks`
  /// performed application authentication.
  #[must_use]
  pub const fn is_authenticated(&self) -> bool {
    return self.authenticated;
  }

  /// Requests client data with classification selected before any data is read.
  ///
  /// The exclusive guard must consume END or CAN and finish before this
  /// context is reused. Its deadline is the smaller of the remaining command
  /// budget and the inquiry timeout.
  ///
  /// # Errors
  /// Encoding, state, write, or timeout failures invalidate the session.
  /// Cancellation after polling closes the channel.
  ///
  /// # Panics
  /// Requires a Tokio runtime with time enabled.
  pub async fn inquire(
    &mut self,
    keyword: &str,
    args: &[u8],
    sensitivity: Sensitivity,
  ) -> Result<ServerInquiry<'_>, HandlerError> {
    let deadline = Instant::now()
      .checked_add(self.options.inquiry_timeout)
      .unwrap_or(self.deadline)
      .min(self.deadline);
    return ServerInquiry::begin(
      self.channel,
      self.machine,
      deadline,
      self.options.max_inquiry_bytes,
      assuan_protocol::ServerLine::Inquire {
        keyword,
        args,
      },
      sensitivity,
    )
    .await;
  }

  /// Sends informational status without completing the command.
  ///
  /// # Errors
  /// Invalid fields, state, I/O failure or cancellation invalidate the channel.
  ///
  /// # Panics
  /// Requires a Tokio runtime with time enabled.
  pub async fn send_status(&mut self, keyword: &str, args: &[u8]) -> Result<(), HandlerError> {
    let mut operation = IoOperation {
      channel: self.channel,
      machine: self.machine,
      complete: false,
    };
    let mut output = Box::new(Zeroizing::new([0; MAX_LINE_BYTES]));
    let len = assuan_protocol::encode_response(
      assuan_protocol::ServerLine::Status {
        keyword,
        args,
      },
      &mut output,
    )
    .map_err(HandlerError::Protocol)?;
    operation.machine.send_response(LineKind::Status).map_err(HandlerError::State)?;
    operation
      .channel
      .write_line(&output[..len], self.deadline, Sensitivity::Secret)
      .await
      .map_err(HandlerError::Transport)?;
    operation.complete = true;
    return Ok(());
  }

  /// Borrows application state owned by this session.
  #[must_use]
  pub fn state(&self) -> &S {
    return self.state;
  }

  /// Exclusively borrows application state for the duration of this borrow.
  #[must_use]
  pub fn state_mut(&mut self) -> &mut S {
    return self.state;
  }

  /// Streams raw bytes as escaped D lines without sending a final response.
  ///
  /// Empty input emits one empty D line. Larger inputs are split at the wire
  /// limit without splitting escapes. All writes share the invocation's total
  /// deadline; the deadline is never restarted per chunk. Temporary storage
  /// and channel buffers are wiped even on cancellation. Caller-owned input
  /// remains the caller's responsibility.
  ///
  /// Once polled, cancellation or failure invalidates the protocol state and
  /// closes the channel, because a prefix may already have reached the peer.
  /// An unpolled future has no effect.
  ///
  /// # Errors
  /// Returns state, protocol, transport, or deadline errors. The connection
  /// cannot be reused after an error, even if the handler catches it.
  ///
  /// # Panics
  /// Requires a Tokio runtime with time enabled.
  pub async fn send_data(&mut self, data: &[u8]) -> Result<(), HandlerError> {
    let mut operation = IoOperation {
      channel: self.channel,
      machine: self.machine,
      complete: false,
    };
    operation.machine.send_response(LineKind::Data).map_err(HandlerError::State)?;
    let mut output = Zeroizing::new([0_u8; MAX_LINE_BYTES]);
    let mut remaining = data;
    loop {
      let (consumed, written) =
        encode_data_chunk(remaining, &mut output).map_err(HandlerError::Protocol)?;
      operation
        .channel
        .write_line(&output[..written], self.deadline, Sensitivity::Secret)
        .await
        .map_err(HandlerError::Transport)?;
      output.zeroize();
      remaining = &remaining[consumed..];
      if remaining.is_empty() {
        break;
      }
    }
    operation.complete = true;
    return Ok(());
  }
}

impl<S: Send + 'static> fmt::Debug for CommandContext<'_, S> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f.debug_struct("CommandContext").finish_non_exhaustive();
  }
}

pub(crate) struct IoOperation<'a> {
  pub channel: &'a mut Channel,
  pub machine: &'a mut ServerMachine,
  pub complete: bool,
}

impl Drop for IoOperation<'_> {
  fn drop(&mut self) {
    if !self.complete {
      self.machine.invalidate();
      self.channel.close();
    }
  }
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
