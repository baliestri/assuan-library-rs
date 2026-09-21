use core::fmt;

use zeroize::{Zeroize, Zeroizing};

use crate::ProtocolError;

/// Maximum Assuan line length in bytes, including its LF or CRLF terminator.
pub const MAX_LINE_BYTES: usize = 1000;

/// Incrementally assembles one line in fixed storage, without heap allocation.
///
/// Unconsumed input belongs to the caller. A complete line remains available
/// until [`Self::clear`]. Storage is wiped on clear, framing failure, and
/// normal drop. As with other stack values, moving this buffer may leave
/// earlier copies; pinning or owning protected I/O storage is the transport's
/// concern. Debug output never includes buffered bytes.
pub struct LineBuffer {
  storage: Zeroizing<[u8; MAX_LINE_BYTES]>,
  len: usize,
  ready: bool,
  content_len: usize,
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
      content_len: 0,
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
        self.content_len = self.len - 1;
        if self.content_len > 0 && self.storage[self.content_len - 1] == b'\r' {
          self.content_len -= 1;
        }
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
    return Some(&self.storage[..self.content_len]);
  }

  /// Mutably borrows a complete line for in-place decoding, excluding CRLF/LF.
  ///
  /// The borrow prevents feeding or clearing the buffer until it ends. Bytes
  /// outside the slice, including terminators, remain inaccessible and are
  /// wiped together with the rest of the storage by [`Self::clear`].
  #[must_use]
  pub fn line_mut(&mut self) -> Option<&mut [u8]> {
    let len = self.line()?.len();
    return Some(&mut self.storage[..len]);
  }

  /// Wipes all storage and resets local framing state for the next line.
  pub fn clear(&mut self) {
    self.storage[..].zeroize();
    self.len = 0;
    self.ready = false;
    self.content_len = 0;
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
  fn mutable_decoding_and_reuse_leave_no_old_bytes() {
    let mut buffer = LineBuffer::new();
    buffer.feed(b"D secret%00\n").unwrap();
    let line = buffer.line_mut().unwrap();
    line.fill(b'\r');
    assert_eq!(buffer.line().unwrap().len(), 11);
    assert_eq!(buffer.line_mut().unwrap().len(), 11);
    buffer.clear();
    buffer.feed(b"OK\n").unwrap();
    assert_eq!(buffer.line_mut().unwrap(), b"OK");
    assert!(buffer.storage[3..].iter().all(|byte| return *byte == 0));
  }

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
