use std::fmt;

use assuan_protocol::{LineKind, MAX_LINE_BYTES, Sensitivity, ServerMachine, encode_data_chunk};
use assuan_transport::Channel;
use tokio::time::Instant;
use zeroize::{Zeroize, Zeroizing};

use crate::HandlerError;

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
}

impl<S: Send + 'static> CommandContext<'_, S> {
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
    let mut operation = SendOperation {
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

struct SendOperation<'a> {
  channel: &'a mut Channel,
  machine: &'a mut ServerMachine,
  complete: bool,
}

impl Drop for SendOperation<'_> {
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
