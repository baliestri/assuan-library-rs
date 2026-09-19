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
