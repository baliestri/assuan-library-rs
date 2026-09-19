use crate::LimitError;

pub(crate) fn extended_length(
  current: usize,
  additional: usize,
  capacity: usize,
) -> Result<usize, LimitError> {
  let length = current.checked_add(additional).ok_or(LimitError::LengthOverflow)?;

  if length > capacity {
    return Err(LimitError::CapacityExceeded);
  }

  return Ok(length);
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn addition_checks_overflow_before_capacity() {
    assert_eq!(extended_length(usize::MAX, 1, usize::MAX), Err(LimitError::LengthOverflow));
  }
}
