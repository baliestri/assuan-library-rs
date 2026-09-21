use std::fmt;

use assuan_protocol::{MAX_LINE_BYTES, ProtocolError, ServerLine, StateError, encode_response};
use assuan_transport::TransportError;
use thiserror::Error;
use zeroize::Zeroizing;

/// A command registration failure. Existing entries remain unchanged.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RegistryError {
  /// The name is invalid or cannot fit in one command line.
  #[error("invalid handler name")]
  InvalidName,
  /// A built-in command owns this name.
  #[error("handler name is reserved: {0}")]
  Reserved(String),
  /// Another handler already owns this name.
  #[error("handler already registered: {0}")]
  Duplicate(String),
  /// The description contains CR, LF, or NUL.
  #[error("invalid handler description")]
  InvalidDescription,
}

/// A handler failure with payload-free diagnostic formatting.
///
/// Remote text is explicitly public wire content; never put secrets in it.
/// Both Debug and Display omit remote messages and nested error details.
/// Technical causes remain available through `std::error::Error::source`;
/// applications must apply their own redaction before logging those sources.
#[derive(Error)]
pub enum HandlerError {
  /// An application error intended for the peer.
  ///
  /// Prefer [`Self::remote`]. Because fields are public, the session runner
  /// must call [`Self::validate_remote`] again immediately before encoding a
  /// final.
  #[error("remote handler error ({code})")]
  Remote {
    /// The unsigned Assuan error code, including any application source bits.
    code: u32,
    /// Public, single-line error text fitting within the protocol wire limit.
    message: String,
  },
  /// An internal failure whose details must never be sent to the peer.
  #[error("internal handler failure")]
  Internal,
  /// I/O failed; the connection is unusable.
  #[error("handler transport failure")]
  Transport(#[source] TransportError),
  /// Protocol encoding or parsing failed.
  #[error("handler protocol failure")]
  Protocol(#[source] ProtocolError),
  /// The operation is not legal in the current protocol state.
  #[error("handler state failure")]
  State(#[source] StateError),
  /// A decoded inquiry exceeded its configured byte budget.
  #[error("handler resource limit exceeded")]
  Limit(#[source] assuan_protocol::LimitError),
}

impl HandlerError {
  /// Creates a remote error after validating its complete encoded wire line.
  ///
  /// Empty text is allowed. The byte budget includes `ERR `, decimal code,
  /// optional separator, and LF; UTF-8 text is measured in bytes.
  ///
  /// # Errors
  /// Returns a protocol error for CR, LF, NUL, or an oversized response.
  pub fn remote(code: u32, message: &str) -> Result<Self, ProtocolError> {
    validate_message(code, message)?;
    return Ok(Self::Remote {
      code,
      message: message.to_owned(),
    });
  }

  /// Checks remote fields, including values constructed directly or mutated.
  ///
  /// Non-remote variants need no text validation and return success. This
  /// method does not decide whether the session may send a final response.
  ///
  /// # Errors
  /// Returns a protocol error if the complete ERR line would be invalid.
  pub fn validate_remote(&self) -> Result<(), ProtocolError> {
    if let Self::Remote {
      code,
      message,
    } = self
    {
      validate_message(*code, message)?;
    }
    return Ok(());
  }
}

fn validate_message(code: u32, message: &str) -> Result<(), ProtocolError> {
  let mut output = Zeroizing::new([0_u8; MAX_LINE_BYTES]);
  encode_response(
    ServerLine::Err {
      code,
      text: message.as_bytes(),
    },
    &mut output,
  )?;
  return Ok(());
}

impl fmt::Debug for HandlerError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return match self {
      Self::Remote {
        code,
        ..
      } => f.debug_struct("Remote").field("code", code).finish_non_exhaustive(),
      Self::Internal => f.write_str("Internal"),
      Self::Transport(_) => f.write_str("Transport { .. }"),
      Self::Protocol(_) => f.write_str("Protocol { .. }"),
      Self::State(_) => f.write_str("State { .. }"),
      Self::Limit(_) => f.write_str("Limit { .. }"),
    };
  }
}

/// Terminal session failures. Diagnostics never include application payloads.
///
/// Explicit error sources may expose application-supplied I/O details; redact
/// those before logging. Ordinary remote command errors are sent to the peer
/// and do not end a session.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ServerError {
  /// Channel I/O, framing, or an operation deadline failed.
  #[error("server transport failure")]
  Transport(#[from] TransportError),
  /// Wire encoding or parsing failed.
  #[error("server protocol failure")]
  Protocol(#[from] ProtocolError),
  /// The session is incomplete or unusable.
  #[error("server state failure")]
  State(#[from] StateError),
  /// A fatal handler operation, authentication, or the close hook failed.
  #[error("server handler or hook failure")]
  Handler(#[source] HandlerError),
  /// Options have a zero limit, unsupported concurrency, or an unrepresentable
  /// deadline.
  #[error("invalid server options")]
  InvalidOptions,
}

// Numeric codes from libgpg-error src/err-codes.h.in:
// https://github.com/gpg/libgpg-error/blob/master/src/err-codes.h.in
pub(crate) const INTERNAL: u32 = 63;
pub(crate) const UNKNOWN_OPTION: u32 = 174;
pub(crate) const UNKNOWN_COMMAND: u32 = 175;
pub(crate) const INVALID_VALUE: u32 = 55;
