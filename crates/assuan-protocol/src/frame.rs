use core::fmt;
use zeroize::{Zeroize, Zeroizing};

use crate::ProtocolError;

/// Maximum Assuan line length in bytes, including its LF or CRLF terminator.
pub const MAX_LINE_BYTES: usize = 1000;

/// Incrementally assembles one line in fixed storage, without heap allocation.
///
/// Unconsumed input belongs to the caller. A complete line remains available
/// until [`Self::clear`]. Storage is wiped on clear, framing failure, and normal
/// drop. As with other stack values, moving this buffer may leave earlier
/// copies; pinning or owning protected I/O storage is the transport's concern.
/// Debug output never includes buffered bytes.
pub struct LineBuffer {
  storage: Zeroizing<[u8; MAX_LINE_BYTES]>,
  len: usize,
  ready: bool,
  failed: bool,
}

impl LineBuffer {
  /// Creates an empty, zero-initialized line buffer.
  #[must_use]
  pub fn new() -> Self {
    return Self {
      storage: Zeroizing::new([0; MAX_LINE_BYTES]),
      len: 0,
      ready: false,
      failed: false,
    };
  }

  /// Consumes bytes through at most the first LF and returns the amount used.
  ///
  /// Returns zero when a line is already ready or `input` is empty. Both LF
  /// and CRLF count toward the wire limit. A CR not followed by LF remains in
  /// the line for the protocol parser to reject.
  ///
  /// # Errors
  /// Returns [`ProtocolError::LineTooLong`] as soon as a line cannot fit its
  /// terminator within the limit. Failure wipes the storage and remains sticky
  /// until clear. The connection owner must treat framing failures as fatal;
  /// clearing this local buffer cannot resynchronize an arbitrary byte stream.
  pub fn feed(&mut self, input: &[u8]) -> Result<usize, ProtocolError> {
    if self.failed {
      return Err(ProtocolError::LineTooLong);
    }
    if self.ready {
      return Ok(0);
    }
    for (index, byte) in input.iter().enumerate() {
      self.storage[self.len] = *byte;
      self.len += 1;
      if *byte == b'\n' {
        self.ready = true;
        return Ok(index + 1);
      }
      if self.len == MAX_LINE_BYTES {
        self.clear();
        self.failed = true;
        return Err(ProtocolError::LineTooLong);
      }
    }
    return Ok(input.len());
  }

  /// Borrows a complete line, excluding LF and its optional preceding CR.
  #[must_use]
  pub fn line(&self) -> Option<&[u8]> {
    if !self.ready {
      return None;
    }
    let mut end = self.len - 1;
    if end > 0 && self.storage[end - 1] == b'\r' {
      end -= 1;
    }
    return Some(&self.storage[..end]);
  }

  /// Wipes all storage and resets local framing state for the next line.
  pub fn clear(&mut self) {
    self.storage[..].zeroize();
    self.len = 0;
    self.ready = false;
    self.failed = false;
  }

  /// Checks whether EOF occurred on a line boundary.
  ///
  /// A complete, unconsumed line is valid here; its protocol meaning must still
  /// be checked by the session. This method does not imply transaction success.
  ///
  /// # Errors
  /// Returns [`ProtocolError::UnexpectedEof`] for an incomplete line or
  /// [`ProtocolError::LineTooLong`] if framing has already failed.
  pub fn finish_eof(&self) -> Result<(), ProtocolError> {
    if self.failed {
      return Err(ProtocolError::LineTooLong);
    }
    if self.len != 0 && !self.ready {
      return Err(ProtocolError::UnexpectedEof);
    }
    return Ok(());
  }
}

impl Default for LineBuffer {
  fn default() -> Self {
    return Self::new();
  }
}

impl fmt::Debug for LineBuffer {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f
      .debug_struct("LineBuffer")
      .field("len", &self.len)
      .field("ready", &self.ready)
      .field("failed", &self.failed)
      .finish_non_exhaustive();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn clear_and_overflow_wipe_every_byte() {
    let mut buffer = LineBuffer::new();
    buffer.feed(b"secret\n").unwrap();
    buffer.clear();
    assert!(buffer.storage.iter().all(|byte| return *byte == 0));
    assert_eq!(buffer.feed(&[b'x'; MAX_LINE_BYTES]), Err(ProtocolError::LineTooLong));
    assert!(buffer.storage.iter().all(|byte| return *byte == 0));
  }
}
