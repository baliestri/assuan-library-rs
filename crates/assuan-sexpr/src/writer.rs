use alloc::vec::Vec;

use crate::{EncodeError, ParseLimits, Sexpr};

struct Layout {
  bytes: usize,
  nodes: usize,
}

enum Pending<'tree, 'input> {
  Value(&'tree Sexpr<'input>),
  CloseList,
}

impl Sexpr<'_> {
  /// Copies this expression into its canonical binary representation.
  ///
  /// Lengths count bytes, and output contains no extra whitespace. The
  /// default [`ParseLimits`] apply to atoms, nodes, nesting and encoded size,
  /// including trees constructed directly instead of parsed. Traversal is
  /// iterative. This allocates output and temporary traversal storage, but
  /// does not allocate intermediate strings.
  ///
  /// The returned bytes are ordinary owned data, not a secret buffer.
  ///
  /// # Errors
  ///
  /// Returns [`EncodeError`] if a limit is exceeded, a size calculation
  /// overflows, or output/traversal storage cannot be reserved.
  ///
  /// # Examples
  ///
  /// ```
  /// use assuan_sexpr::Sexpr;
  /// let value = Sexpr::Atom("é".as_bytes());
  /// assert_eq!(value.to_canonical()?, b"2:\xc3\xa9");
  /// # Ok::<(), assuan_sexpr::EncodeError>(())
  /// ```
  pub fn to_canonical(&self) -> Result<Vec<u8>, EncodeError> {
    let mut output = Vec::new();
    self.write_canonical(&mut output)?;
    return Ok(output);
  }

  /// Appends this expression's canonical bytes to an existing buffer.
  ///
  /// Applies default [`ParseLimits`] to this expression. The output-size
  /// limit includes bytes already in `output`. Atom bytes are copied;
  /// temporary traversal storage and additional output capacity may be
  /// allocated. No intermediate strings are built.
  ///
  /// # Errors
  ///
  /// Returns [`EncodeError`] on an exceeded limit, arithmetic overflow or
  /// failed reservation. All fallible work occurs before appending: on
  /// error, existing output bytes and length are unchanged.
  pub fn write_canonical(&self, output: &mut Vec<u8>) -> Result<(), EncodeError> {
    let limits = ParseLimits::default();
    let layout = measure(self, limits)?;
    let final_len = output.len().checked_add(layout.bytes).ok_or(EncodeError::LengthOverflow)?;
    if final_len > limits.max_input() {
      return Err(EncodeError::SizeLimit);
    }

    let mut pending = Vec::new();
    pending.try_reserve(layout.nodes).map_err(|_| return EncodeError::AllocationFailed)?;
    output.try_reserve(layout.bytes).map_err(|_| return EncodeError::AllocationFailed)?;
    pending.push(Pending::Value(self));

    while let Some(item) = pending.pop() {
      match item {
        Pending::Value(Self::Atom(bytes)) => {
          write_length(bytes.len(), output);
          output.push(b':');
          output.extend_from_slice(bytes);
        }
        Pending::Value(Self::List(children)) => {
          output.push(b'(');
          pending.push(Pending::CloseList);
          pending.extend(children.iter().rev().map(Pending::Value));
        }
        Pending::CloseList => output.push(b')'),
      }
    }
    debug_assert_eq!(output.len(), final_len);
    return Ok(());
  }
}

fn measure(value: &Sexpr<'_>, limits: ParseLimits) -> Result<Layout, EncodeError> {
  let mut pending = Vec::new();
  pending.try_reserve(1).map_err(|_| return EncodeError::AllocationFailed)?;
  pending.push((value, 0));
  let mut layout = Layout {
    bytes: 0,
    nodes: 0,
  };

  while let Some((value, depth)) = pending.pop() {
    if layout.nodes >= limits.max_nodes() {
      return Err(EncodeError::NodeLimit);
    }
    layout.nodes += 1;
    let added = match value {
      Sexpr::Atom(bytes) => {
        if bytes.len() > limits.max_atom() {
          return Err(EncodeError::AtomLimit);
        }
        decimal_digits(bytes.len())
          .checked_add(1)
          .and_then(|prefix| return prefix.checked_add(bytes.len()))
          .ok_or(EncodeError::LengthOverflow)?
      }
      Sexpr::List(children) => {
        if depth >= limits.max_depth() {
          return Err(EncodeError::DepthLimit);
        }
        let minimum_nodes = layout
          .nodes
          .checked_add(pending.len())
          .and_then(|count| return count.checked_add(children.len()))
          .ok_or(EncodeError::LengthOverflow)?;
        if minimum_nodes > limits.max_nodes() {
          return Err(EncodeError::NodeLimit);
        }
        pending.try_reserve(children.len()).map_err(|_| return EncodeError::AllocationFailed)?;
        pending.extend(children.iter().rev().map(|child| return (child, depth + 1)));
        2
      }
    };
    layout.bytes = layout.bytes.checked_add(added).ok_or(EncodeError::LengthOverflow)?;
    if layout.bytes > limits.max_input() {
      return Err(EncodeError::SizeLimit);
    }
  }
  return Ok(layout);
}

fn decimal_digits(mut value: usize) -> usize {
  let mut count = 1;
  while value >= 10 {
    value /= 10;
    count += 1;
  }
  return count;
}

fn write_length(mut value: usize, output: &mut Vec<u8>) {
  let mut digits = [0_u8; 40];
  let mut position = digits.len();
  loop {
    position -= 1;
    digits[position] = b'0' + u8::try_from(value % 10).expect("decimal digit fits u8");
    value /= 10;
    if value == 0 {
      break;
    }
  }
  output.extend_from_slice(&digits[position..]);
}
