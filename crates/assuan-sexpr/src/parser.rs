use alloc::vec::Vec;

use crate::{ParseError, ParseErrorKind, ParseLimits, Sexpr};

/// Parses the first canonical expression and returns the number of bytes read.
///
/// Atoms borrow `input`; list nodes allocate child storage. The parser uses an
/// explicit stack and checks lengths and resource limits before reserving
/// storage. The input-byte limit applies to the entire supplied slice, even
/// when this function parses only its first expression.
///
/// Whitespace outside an atom is not canonical and is not skipped. Bytes after
/// the first complete expression are left unexamined (apart from the input
/// size check). Use [`parse_complete`] to reject trailing bytes.
///
/// # Errors
///
/// Returns [`ParseError`] for invalid syntax, truncated input, integer overflow,
/// an exceeded limit or failed reservation of list storage. The error includes
/// its byte offset but no copy of the input.
///
/// # Examples
///
/// ```
/// use assuan_sexpr::{parse_prefix, ParseLimits, Sexpr};
///
/// let (value, consumed) = parse_prefix(b"1:a1:b", ParseLimits::default())?;
/// assert_eq!(value, Sexpr::Atom(b"a"));
/// assert_eq!(consumed, 3);
/// # Ok::<(), assuan_sexpr::ParseError>(())
/// ```
pub fn parse_prefix(input: &[u8], limits: ParseLimits) -> Result<(Sexpr<'_>, usize), ParseError> {
  if input.len() > limits.max_input() {
    return Err(ParseError::at(ParseErrorKind::InputLimit, limits.max_input()));
  }

  let mut lists: Vec<Vec<Sexpr<'_>>> = Vec::new();
  let mut position = 0;
  let mut nodes = 0;

  loop {
    let byte = *input
      .get(position)
      .ok_or_else(|| return ParseError::at(ParseErrorKind::UnexpectedEof, position))?;
    let value = match byte {
      b'(' => {
        if lists.len() >= limits.max_depth() {
          return Err(ParseError::at(ParseErrorKind::DepthLimit, position));
        }
        count_node(&mut nodes, limits, position)?;
        lists
          .try_reserve(1)
          .map_err(|_| return ParseError::at(ParseErrorKind::AllocationFailed, position))?;
        lists.push(Vec::new());
        position += 1;
        continue;
      }
      b')' => {
        let children = lists
          .pop()
          .ok_or_else(|| return ParseError::at(ParseErrorKind::UnexpectedToken, position))?;
        position += 1;
        Sexpr::List(children)
      }
      b'0'..=b'9' => {
        count_node(&mut nodes, limits, position)?;
        Sexpr::Atom(parse_atom(input, &mut position, limits)?)
      }
      _ => return Err(ParseError::at(ParseErrorKind::UnexpectedToken, position)),
    };

    if let Some(parent) = lists.last_mut() {
      parent
        .try_reserve(1)
        .map_err(|_| return ParseError::at(ParseErrorKind::AllocationFailed, position))?;
      parent.push(value);
    } else {
      return Ok((value, position));
    }
  }
}

/// Parses exactly one canonical S-expression, borrowing atom payloads.
///
/// This applies the same syntax and limits as [`parse_prefix`], and additionally
/// requires that every input byte belongs to the expression. It does not trim
/// whitespace before or after it. Only list structure is allocated.
///
/// # Errors
///
/// Returns the errors documented by [`parse_prefix`], or
/// [`ParseErrorKind::TrailingData`] at the first byte after the expression.
///
/// # Examples
///
/// ```
/// use assuan_sexpr::{parse_complete, ParseLimits, Sexpr};
///
/// let value = parse_complete(b"(3:foo0:)", ParseLimits::default())?;
/// assert_eq!(value, Sexpr::List(vec![Sexpr::Atom(b"foo"), Sexpr::Atom(b"")]));
/// # Ok::<(), assuan_sexpr::ParseError>(())
/// ```
pub fn parse_complete(input: &[u8], limits: ParseLimits) -> Result<Sexpr<'_>, ParseError> {
  let (value, consumed) = parse_prefix(input, limits)?;

  if consumed != input.len() {
    return Err(ParseError::at(ParseErrorKind::TrailingData, consumed));
  }

  return Ok(value);
}

fn count_node(nodes: &mut usize, limits: ParseLimits, position: usize) -> Result<(), ParseError> {
  if *nodes >= limits.max_nodes() {
    return Err(ParseError::at(ParseErrorKind::NodeLimit, position));
  }

  *nodes += 1;

  return Ok(());
}

fn parse_atom<'a>(
  input: &'a [u8],
  position: &mut usize,
  limits: ParseLimits,
) -> Result<&'a [u8], ParseError> {
  let start = *position;
  let mut length = 0_usize;
  while let Some(&byte) = input.get(*position) {
    if !byte.is_ascii_digit() {
      break;
    }

    if *position > start && input[start] == b'0' {
      return Err(ParseError::at(ParseErrorKind::InvalidLength, *position));
    }

    let digit = usize::from(byte - b'0');
    length = length
      .checked_mul(10)
      .and_then(|value| return value.checked_add(digit))
      .ok_or_else(|| return ParseError::at(ParseErrorKind::LengthOverflow, *position))?;
    *position += 1;
  }

  match input.get(*position) {
    Some(b':') => *position += 1,
    Some(_) => return Err(ParseError::at(ParseErrorKind::ExpectedColon, *position)),
    None => return Err(ParseError::at(ParseErrorKind::UnexpectedEof, *position)),
  }

  if length > limits.max_atom() {
    return Err(ParseError::at(ParseErrorKind::AtomLimit, start));
  }

  let end = (*position)
    .checked_add(length)
    .ok_or_else(|| return ParseError::at(ParseErrorKind::LengthOverflow, start))?;
  let payload = input
    .get(*position..end)
    .ok_or_else(|| return ParseError::at(ParseErrorKind::UnexpectedEof, input.len()))?;
  *position = end;

  return Ok(payload);
}
