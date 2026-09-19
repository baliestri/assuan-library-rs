use core::fmt;

/// A resource limit or allocation failure, without payload contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LimitError {
  /// The allocator could not reserve the requested storage.
  AllocationFailed,
  /// Combining lengths would overflow the platform's address space.
  LengthOverflow,
  /// The operation would exceed the buffer's fixed capacity.
  CapacityExceeded,
}

impl fmt::Display for LimitError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f.write_str(match self {
      Self::AllocationFailed => "storage allocation failed",
      Self::LengthOverflow => "buffer length overflow",
      Self::CapacityExceeded => "buffer capacity exceeded",
    });
  }
}

impl core::error::Error for LimitError {}

/// A malformed protocol field, without any input bytes in its diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProtocolError {
  /// A line contains a forbidden NUL, CR, or LF byte.
  InvalidLine,
  /// A command name or keyword is missing or violates its ASCII grammar.
  InvalidToken,
  /// The response category or its separators are invalid.
  InvalidResponse,
  /// A remote error code is missing, nondecimal, or larger than `u32::MAX`.
  InvalidErrorCode,
  /// A percent escape is incomplete or contains a nonhexadecimal digit.
  InvalidEscape,
  /// A line cannot fit within the wire limit including its terminator.
  LineTooLong,
  /// EOF arrived before the current line was terminated.
  UnexpectedEof,
}

impl fmt::Display for ProtocolError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f.write_str(match self {
      Self::InvalidLine => "invalid byte in protocol line",
      Self::InvalidToken => "invalid protocol token",
      Self::InvalidResponse => "invalid server response",
      Self::InvalidErrorCode => "invalid remote error code",
      Self::InvalidEscape => "invalid percent escape",
      Self::LineTooLong => "protocol line exceeds wire limit",
      Self::UnexpectedEof => "incomplete protocol line at EOF",
    });
  }
}

impl core::error::Error for ProtocolError {}
