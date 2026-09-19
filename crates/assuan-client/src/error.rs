use assuan_protocol::{ProtocolError, StateError};
use assuan_transport::TransportError;

/// Client failures without remote text, command arguments, or payload contents.
///
/// Transport sources are retained explicitly; their underlying I/O sources may
/// contain external diagnostics. Inspect those only where appropriate.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ClientError {
  /// Transport I/O, framing, discovery, or timeout failed.
  #[error("client transport failed")]
  Transport(#[from] TransportError),
  /// A command or server response failed wire validation.
  #[error("invalid client protocol data")]
  Protocol(#[from] ProtocolError),
  /// The session cannot perform the requested state transition.
  #[error("invalid client session state")]
  State(#[from] StateError),
  /// A normal remote command error; a completed transaction remains reusable.
  #[error("remote command failed with code {code}")]
  Remote {
    /// Complete unsigned remote code, including its source bits.
    code: u32,
  },
  /// A remote ERR rejected the initial greeting; the stream is closed.
  #[error("server rejected greeting with code {code}")]
  GreetingRejected {
    /// Complete unsigned remote code.
    code: u32,
  },
  /// A duration is zero or cannot be represented as a deadline.
  #[error("invalid client options")]
  InvalidOptions,
  /// Finish was requested before receiving a final OK or ERR.
  #[error("transaction is incomplete")]
  Incomplete,
  /// Interactive inquiry support is not available in this implementation stage.
  #[error("interactive inquiry is not supported")]
  UnsupportedInquiry,
}
