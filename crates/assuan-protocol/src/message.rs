use core::fmt;

use crate::{
  ProtocolError,
  command::{split_field, validate_line, validate_token},
};

/// A server response borrowing its original wire-format fields.
///
/// Data remains percent-encoded until explicitly decoded. Debug output includes
/// only the response category, lengths, and an error code when applicable.
/// Public variants can be constructed directly; parsers validate wire input.
pub enum ServerLine<'a> {
  /// Successful completion with optional diagnostic text.
  Ok(&'a [u8]),
  /// Failed completion carrying a libgpg-error code and optional text.
  Err {
    /// Numeric remote error code, including its source bits.
    code: u32,
    /// Original diagnostic bytes without the separating space.
    text: &'a [u8],
  },
  /// Percent-encoded data, preserving spaces after the first separator.
  Data(&'a [u8]),
  /// Informational status that does not complete the operation.
  Status {
    /// ASCII keyword beginning with a letter or underscore.
    keyword: &'a str,
    /// Unmodified keyword-specific arguments.
    args: &'a [u8],
  },
  /// Request for client data, terminated by END or CAN.
  Inquire {
    /// ASCII keyword beginning with a letter or underscore.
    keyword: &'a str,
    /// Unmodified inquiry-specific arguments.
    args: &'a [u8],
  },
  /// A partial end of server data; not a final OK or ERR.
  End,
  /// Comment bytes following `#`, including any space after it.
  Comment(&'a [u8]),
  /// An empty line.
  Empty,
}

/// Parses a server line without its LF or CRLF terminator, without allocation.
///
/// A single separating space is consumed for each field; payload spaces and
/// binary bytes remain unchanged. Unknown status and inquiry keywords are
/// accepted. Line-length limits belong to framing, not this parser.
///
/// # Errors
/// Returns [`ProtocolError::InvalidLine`] for embedded NUL, CR, or LF;
/// [`ProtocolError::InvalidResponse`] for an unknown or malformed response;
/// [`ProtocolError::InvalidToken`] for invalid keywords; or
/// [`ProtocolError::InvalidErrorCode`] for a missing, nondecimal, or
/// overflowing error code. Data escapes are validated separately when decoded.
pub fn parse_server_line(line: &[u8]) -> Result<ServerLine<'_>, ProtocolError> {
  validate_line(line)?;

  if line.is_empty() {
    return Ok(ServerLine::Empty);
  }

  if let Some(comment) = line.strip_prefix(b"#") {
    return Ok(ServerLine::Comment(comment));
  }

  if line == b"OK" {
    return Ok(ServerLine::Ok(&[]));
  }

  if let Some(text) = line.strip_prefix(b"OK ") {
    return Ok(ServerLine::Ok(text));
  }

  if let Some(fields) = line.strip_prefix(b"ERR ") {
    let (number, text) = split_field(fields);
    let mut code = 0_u32;

    if number.is_empty() {
      return Err(ProtocolError::InvalidErrorCode);
    }

    for digit in number {
      if !digit.is_ascii_digit() {
        return Err(ProtocolError::InvalidErrorCode);
      }

      code = code
        .checked_mul(10)
        .and_then(|value| return value.checked_add(u32::from(digit - b'0')))
        .ok_or(ProtocolError::InvalidErrorCode)?;
    }

    return Ok(ServerLine::Err {
      code,
      text,
    });
  }

  if let Some(data) = line.strip_prefix(b"D ") {
    return Ok(ServerLine::Data(data));
  }

  if let Some(fields) = line.strip_prefix(b"S ") {
    let (keyword, args) = parse_keyword(fields)?;
    return Ok(ServerLine::Status {
      keyword,
      args,
    });
  }

  if let Some(fields) = line.strip_prefix(b"INQUIRE ") {
    let (keyword, args) = parse_keyword(fields)?;
    return Ok(ServerLine::Inquire {
      keyword,
      args,
    });
  }

  if line == b"END" {
    return Ok(ServerLine::End);
  }
  return Err(ProtocolError::InvalidResponse);
}

fn parse_keyword(fields: &[u8]) -> Result<(&str, &[u8]), ProtocolError> {
  let (keyword, args) = split_field(fields);
  validate_token(keyword)?;

  if !keyword[0].is_ascii_alphabetic() && keyword[0] != b'_' {
    return Err(ProtocolError::InvalidToken);
  }

  let keyword = core::str::from_utf8(keyword).map_err(|_| return ProtocolError::InvalidToken)?;
  return Ok((keyword, args));
}

impl fmt::Debug for ServerLine<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Ok(bytes) => return field_debug(f, "Ok", bytes),
      Self::Err {
        code,
        text,
      } => {
        return f
          .debug_struct("Err")
          .field("code", code)
          .field("text_len", &text.len())
          .finish_non_exhaustive();
      }
      Self::Data(bytes) => return field_debug(f, "Data", bytes),
      Self::Status {
        keyword,
        args,
      }
      | Self::Inquire {
        keyword,
        args,
      } => {
        let name = if matches!(self, Self::Status { .. }) {
          "Status"
        } else {
          "Inquire"
        };
        return f
          .debug_struct(name)
          .field("keyword_len", &keyword.len())
          .field("args_len", &args.len())
          .finish_non_exhaustive();
      }
      Self::End => return f.write_str("End"),
      Self::Comment(bytes) => return field_debug(f, "Comment", bytes),
      Self::Empty => return f.write_str("Empty"),
    }
  }
}

fn field_debug(f: &mut fmt::Formatter<'_>, name: &str, bytes: &[u8]) -> fmt::Result {
  return f.debug_struct(name).field("len", &bytes.len()).finish_non_exhaustive();
}
