use crate::ProtocolError;

/// Decodes percent-escaped data in place, returning the decoded byte count.
///
/// Hex digits may use either case. All other bytes, including whitespace,
/// binary values, and NUL, are preserved. This function does not parse framing.
/// Only the returned prefix contains decoded data; trailing bytes retain
/// previous contents and must be wiped by the buffer owner when sensitive.
///
/// # Errors
/// Returns [`ProtocolError::InvalidEscape`] for an incomplete or nonhexadecimal
/// escape. Validation happens before mutation, so errors leave input unchanged.
///
/// ```
/// let mut bytes = *b"  %FF%0a";
/// let len = assuan_protocol::decode_data_in_place(&mut bytes)?;
/// assert_eq!(&bytes[..len], b"  \xff\n");
/// # Ok::<(), assuan_protocol::ProtocolError>(())
/// ```
pub fn decode_data_in_place(bytes: &mut [u8]) -> Result<usize, ProtocolError> {
  let mut read = 0;
  while read < bytes.len() {
    if bytes[read] == b'%' {
      if bytes.len() - read < 3 || hex(bytes[read + 1]).is_none() || hex(bytes[read + 2]).is_none()
      {
        return Err(ProtocolError::InvalidEscape);
      }

      read += 3;
    } else {
      read += 1;
    }
  }
  read = 0;
  let mut written = 0;
  while read < bytes.len() {
    if bytes[read] == b'%' {
      // The validation pass guarantees both hex digits exist and parse.
      let high = hex(bytes[read + 1]).unwrap_or(0);
      let low = hex(bytes[read + 2]).unwrap_or(0);
      bytes[written] = high * 16 + low;
      read += 3;
    } else {
      bytes[written] = bytes[read];
      read += 1;
    }

    written += 1;
  }
  return Ok(written);
}

fn hex(byte: u8) -> Option<u8> {
  match byte {
    b'0'..=b'9' => return Some(byte - b'0'),
    b'a'..=b'f' => return Some(byte - b'a' + 10),
    b'A'..=b'F' => return Some(byte - b'A' + 10),
    _ => return None,
  }
}
