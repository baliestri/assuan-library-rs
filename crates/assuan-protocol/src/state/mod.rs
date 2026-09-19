//! Pure, allocation-free session transition validation.

mod client;
mod server;

pub use client::{ClientMachine, ClientState};
pub use server::{ServerMachine, ServerState};

use crate::ServerLine;
use core::fmt;

/// Response categories, without payload bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
  /// Successful final response.
  Ok,
  /// Failed final response.
  Err,
  /// Data segment.
  Data,
  /// Informational status.
  Status,
  /// Request for client-provided data.
  Inquire,
  /// Partial end of data, not final completion.
  End,
  /// Ignorable comment.
  Comment,
  /// Ignorable empty line.
  Empty,
}

impl ServerLine<'_> {
  /// Returns this response's category without exposing or copying its payload.
  #[must_use]
  pub const fn kind(&self) -> LineKind {
    return match self {
      Self::Ok(_) => LineKind::Ok,
      Self::Err {
        ..
      } => LineKind::Err,
      Self::Data(_) => LineKind::Data,
      Self::Status {
        ..
      } => LineKind::Status,
      Self::Inquire {
        ..
      } => LineKind::Inquire,
      Self::End => LineKind::End,
      Self::Comment(_) => LineKind::Comment,
      Self::Empty => LineKind::Empty,
    };
  }
}

/// Client request categories supplied after wire validation.
///
/// `CAN` is inquiry cancellation. The reserved command `CANCEL` must be
/// classified as [`Self::Command`], never [`Self::Can`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
  /// An application or built-in command, including the reserved CANCEL name.
  Command,
  /// Client-provided inquiry data.
  Data,
  /// Successful end of inquiry data (END).
  End,
  /// Cancellation of the current inquiry (CAN).
  Can,
  /// Ignorable comment.
  Comment,
  /// Ignorable empty line.
  Empty,
}

/// A session transition failure containing no command or payload data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StateError {
  /// The event is illegal in the current phase; the session was invalidated.
  UnexpectedEvent,
  /// A local operation was requested in the wrong phase; state is unchanged.
  NotReady,
  /// The session is already invalid or closed and cannot be reused.
  Unusable,
  /// A remote error rejected the greeting; the session was invalidated.
  GreetingRejected,
}

impl fmt::Display for StateError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f.write_str(match self {
      Self::UnexpectedEvent => "unexpected session event",
      Self::NotReady => "session is not ready for this operation",
      Self::Unusable => "session is invalid or closed",
      Self::GreetingRejected => "server rejected the greeting",
    });
  }
}

impl core::error::Error for StateError {}
