use crate::{Command, MAX_LINE_BYTES, ProtocolError, ServerLine, command::validate_line};

/// Encodes a command with an LF terminator into caller-owned storage.
///
/// Arguments are already in wire representation and are not escaped or
/// normalized. Empty arguments omit the separator. Only the returned prefix
/// is written; the caller must protect and wipe the whole output if sensitive.
///
/// # Errors
/// Returns [`ProtocolError::LineTooLong`] if the complete line exceeds the
/// wire limit. Errors leave the output unchanged.
#[expect(
  clippy::needless_pass_by_value,
  reason = "encoding consumes the temporary wire view, not its borrowed bytes"
)]
pub fn encode_command(
  command: Command<'_>,
  output: &mut [u8; MAX_LINE_BYTES],
) -> Result<usize, ProtocolError> {
  if command.args().is_empty() {
    return write_parts(&[command.name().as_bytes()], output);
  }
  return write_parts(&[command.name().as_bytes(), b" ", command.args()], output);
}

/// Encodes as much raw data as fits in one `D ` line, terminated by LF.
///
/// Returns `(input_consumed, output_written)`. Percent, CR, LF, and NUL become
/// uppercase percent escapes; other bytes are copied unchanged. Empty input
/// emits `D \n`. Nonempty input always makes progress. Repeat with the
/// remaining input to stream a larger payload. No escape is split across lines.
/// The output allocation and its cleanup remain the caller's responsibility.
///
/// # Errors
/// This fixed-size implementation always succeeds. The result type matches
/// the other protocol encoders for uniform error handling.
pub fn encode_data_chunk(
  input: &[u8],
  output: &mut [u8; MAX_LINE_BYTES],
) -> Result<(usize, usize), ProtocolError> {
  output[..2].copy_from_slice(b"D ");
  let mut written = 2;
  let mut consumed = 0;
  for byte in input {
    let escaped: Option<&[u8; 3]> = match byte {
      b'%' => Some(b"%25"),
      b'\r' => Some(b"%0D"),
      b'\n' => Some(b"%0A"),
      0 => Some(b"%00"),
      _ => None,
    };
    let cost = if escaped.is_some() {
      3
    } else {
      1
    };
    if written + cost >= MAX_LINE_BYTES {
      break;
    }
    if let Some(escape) = escaped {
      output[written..written + 3].copy_from_slice(escape);
    } else {
      output[written] = *byte;
    }
    written += cost;
    consumed += 1;
  }
  output[written] = b'\n';
  return Ok((consumed, written + 1));
}

/// Validates and encodes a complete server response with an LF terminator.
///
/// [`ServerLine::Data`] contains already percent-encoded bytes, as returned by
/// the parser; use [`encode_data_chunk`] for raw data. Diagnostic text and
/// keyword arguments are copied without additional escaping. Comments include
/// exactly the bytes following `#`. Output is owned and wiped by the caller.
///
/// # Errors
/// Returns a [`ProtocolError`] for invalid line bytes, keyword syntax, data
/// escapes, or length. Validation completes before writing any output bytes.
#[expect(
  clippy::needless_pass_by_value,
  reason = "encoding consumes the temporary wire view, not its borrowed bytes"
)]
pub fn encode_response(
  line: ServerLine<'_>,
  output: &mut [u8; MAX_LINE_BYTES],
) -> Result<usize, ProtocolError> {
  match line {
    ServerLine::Ok(text) => return write_optional(b"OK", text, output),
    ServerLine::Err {
      code,
      text,
    } => {
      let mut digits = [0_u8; 10];
      let mut start = digits.len();
      let mut value = code;
      loop {
        start -= 1;
        digits[start] = b'0' + u8::try_from(value % 10).unwrap_or(0);
        value /= 10;
        if value == 0 {
          break;
        }
      }
      if text.is_empty() {
        return write_parts(&[b"ERR ", &digits[start..]], output);
      }
      return write_parts(&[b"ERR ", &digits[start..], b" ", text], output);
    }
    ServerLine::Data(data) => {
      validate_encoded_data(data)?;
      return write_parts(&[b"D ", data], output);
    }
    ServerLine::Status {
      keyword,
      args,
    } => return write_keyword(b"S ", keyword, args, output),
    ServerLine::Inquire {
      keyword,
      args,
    } => return write_keyword(b"INQUIRE ", keyword, args, output),
    ServerLine::End => return write_parts(&[b"END"], output),
    ServerLine::Comment(text) => return write_parts(&[b"#", text], output),
    ServerLine::Empty => return write_parts(&[], output),
  }
}

fn write_keyword(
  prefix: &[u8],
  keyword: &str,
  args: &[u8],
  output: &mut [u8; MAX_LINE_BYTES],
) -> Result<usize, ProtocolError> {
  crate::command::validate_token(keyword.as_bytes())?;
  if !keyword.as_bytes()[0].is_ascii_alphabetic() && keyword.as_bytes()[0] != b'_' {
    return Err(ProtocolError::InvalidToken);
  }
  if args.is_empty() {
    return write_parts(&[prefix, keyword.as_bytes()], output);
  }
  return write_parts(&[prefix, keyword.as_bytes(), b" ", args], output);
}

fn write_optional(
  prefix: &[u8],
  text: &[u8],
  output: &mut [u8; MAX_LINE_BYTES],
) -> Result<usize, ProtocolError> {
  if text.is_empty() {
    return write_parts(&[prefix], output);
  }
  return write_parts(&[prefix, b" ", text], output);
}

fn write_parts(parts: &[&[u8]], output: &mut [u8; MAX_LINE_BYTES]) -> Result<usize, ProtocolError> {
  let mut total = 1_usize;
  for part in parts {
    validate_line(part)?;
    total = total.checked_add(part.len()).ok_or(ProtocolError::LineTooLong)?;
    if total > MAX_LINE_BYTES {
      return Err(ProtocolError::LineTooLong);
    }
  }
  let mut position = 0;
  for part in parts {
    output[position..position + part.len()].copy_from_slice(part);
    position += part.len();
  }
  output[position] = b'\n';
  return Ok(total);
}

fn validate_encoded_data(data: &[u8]) -> Result<(), ProtocolError> {
  let mut position = 0;
  while position < data.len() {
    if data[position] == b'%' {
      if data.len() - position < 3
        || !data[position + 1].is_ascii_hexdigit()
        || !data[position + 2].is_ascii_hexdigit()
      {
        return Err(ProtocolError::InvalidEscape);
      }
      position += 3;
    } else {
      position += 1;
    }
  }
  return Ok(());
}
