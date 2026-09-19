use std::{fmt, io};

/// Endpoint discovery failure categories with no captured subprocess output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DiscoveryError {
  /// No usable endpoint was found.
  #[error("no endpoint was found")]
  NotFound,
  /// Discovery output is malformed.
  #[error("invalid endpoint discovery output")]
  InvalidOutput,
  /// Discovery output exceeded its configured resource limit.
  #[error("endpoint discovery output limit exceeded")]
  OutputLimit,
}

/// Typed transport failures whose default diagnostics omit external contents.
///
/// I/O sources remain available explicitly through `Error::source` and pattern
/// matching. Those sources may contain paths or custom text; inspect them only
/// where appropriate. Display and Debug of this wrapper never format a source.
#[derive(thiserror::Error)]
#[non_exhaustive]
pub enum TransportError {
  /// An operating-system or custom transport I/O operation failed.
  #[error("transport I/O failed")]
  Io(#[from] io::Error),
  /// The total time allowed for an operation expired.
  #[error("transport operation timed out")]
  Timeout,
  /// The endpoint is unusable for the requested operation.
  #[error("invalid transport endpoint")]
  InvalidEndpoint,
  /// Options cannot be represented or applied.
  #[error("invalid transport options")]
  InvalidOptions,
  /// The requested local access policy could not be enforced.
  #[error("local transport access policy could not be enforced")]
  AccessPolicy,
  /// Endpoint discovery failed without exposing its output.
  #[error("endpoint discovery failed")]
  Discovery(#[from] DiscoveryError),
}

impl fmt::Debug for TransportError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Io(error) => {
        return f.debug_struct("Io").field("kind", &error.kind()).finish_non_exhaustive();
      }
      Self::Timeout => return f.write_str("Timeout"),
      Self::InvalidEndpoint => return f.write_str("InvalidEndpoint"),
      Self::InvalidOptions => return f.write_str("InvalidOptions"),
      Self::AccessPolicy => return f.write_str("AccessPolicy"),
      Self::Discovery(error) => return f.debug_tuple("Discovery").field(error).finish(),
    }
  }
}
