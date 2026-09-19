use core::fmt;

use crate::ProtocolError;

/// A validated command borrowing its name and wire-format arguments.
///
/// Arguments are not trimmed, tokenized, or percent-decoded: interpretation is
/// command-specific. `Debug` reports only lengths. This parser accepts lines
/// without terminators; framing is responsible for enforcing the wire limit.
pub struct Command<'a> {
  name: &'a str,
  args: &'a [u8],
}

impl<'a> Command<'a> {
  /// Validates a command name and arguments already in wire representation.
  ///
  /// Names are nonempty printable ASCII tokens without whitespace or `%`,
  /// and cannot start with `#` (which denotes a comment). Case is preserved.
  /// No escaping or other transformation is applied to arguments.
  ///
  /// # Errors
  /// Returns [`ProtocolError::InvalidToken`] for an invalid name or
  /// [`ProtocolError::InvalidLine`] for NUL, CR, or LF in the arguments.
  pub fn new(name: &'a str, args: &'a [u8]) -> Result<Self, ProtocolError> {
    validate_token(name.as_bytes())?;
    validate_line(args)?;
    return Ok(Self {
      name,
      args,
    });
  }

  /// Parses a command line without its LF or CRLF terminator.
  ///
  /// Exactly the first separating space is removed. Additional spaces and
  /// all argument bytes are preserved, including invalid UTF-8 and escapes.
  ///
  /// # Errors
  /// Returns [`ProtocolError::InvalidLine`] for NUL, CR, or LF, or
  /// [`ProtocolError::InvalidToken`] for a missing or invalid command name.
  pub fn parse(line: &'a [u8]) -> Result<Self, ProtocolError> {
    validate_line(line)?;
    let (name, args) = split_field(line);
    validate_token(name)?;
    let name = core::str::from_utf8(name).map_err(|_| return ProtocolError::InvalidToken)?;
    return Ok(Self {
      name,
      args,
    });
  }

  /// Returns the original command name without case normalization.
  #[must_use]
  pub const fn name(&self) -> &'a str {
    return self.name;
  }

  /// Returns the original wire-format arguments without the first separator.
  #[must_use]
  pub const fn args(&self) -> &'a [u8] {
    return self.args;
  }
}

impl fmt::Debug for Command<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f
      .debug_struct("Command")
      .field("name_len", &self.name.len())
      .field("args_len", &self.args.len())
      .finish_non_exhaustive();
  }
}

pub(crate) fn validate_line(line: &[u8]) -> Result<(), ProtocolError> {
  if line.iter().any(|byte| matches!(byte, 0 | b'\r' | b'\n')) {
    return Err(ProtocolError::InvalidLine);
  }
  return Ok(());
}

pub(crate) fn validate_token(token: &[u8]) -> Result<(), ProtocolError> {
  if token.is_empty()
    || token[0] == b'#'
    || !token.iter().all(|byte| return byte.is_ascii_graphic() && *byte != b'%')
  {
    return Err(ProtocolError::InvalidToken);
  }
  return Ok(());
}

pub(crate) fn split_field(bytes: &[u8]) -> (&[u8], &[u8]) {
  match bytes.iter().position(|byte| return *byte == b' ') {
    Some(index) => return (&bytes[..index], &bytes[index + 1..]),
    None => return (bytes, &[]),
  }
}
