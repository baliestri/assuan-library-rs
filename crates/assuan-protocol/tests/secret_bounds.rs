//! Allocation boundaries, borrowed views, and redacted diagnostics.

use assuan_protocol::{LimitError, PayloadRef, SecretBytes, SecretRef, Sensitivity};

#[test]
fn appends_and_clear_preserve_the_allocation() {
  let mut secret = SecretBytes::with_capacity(8).unwrap();
  let pointer = secret.expose().as_ptr();
  secret.extend_from_slice(b"abc").unwrap();
  secret.extend_from_slice(b"defgh").unwrap();
  assert_eq!(secret.len(), 8);
  assert_eq!(secret.capacity(), 8);
  assert_eq!(secret.expose().as_ptr(), pointer);
  assert_eq!(secret.extend_from_slice(b"overflow"), Err(LimitError::CapacityExceeded));
  assert_eq!(secret.expose(), b"abcdefgh");
  assert_eq!(secret.expose().as_ptr(), pointer);
  secret.clear();
  secret.extend_from_slice(b"new").unwrap();
  assert_eq!(secret.expose(), b"new");
  assert_eq!(secret.expose().as_ptr(), pointer);
  assert_eq!(secret.capacity(), 8);
}

#[test]
fn empty_appends_are_valid_at_any_capacity() {
  let mut empty = SecretBytes::with_capacity(0).unwrap();
  empty.extend_from_slice(b"").unwrap();
  empty.clear();
  assert_eq!(empty.capacity(), 0);
  assert!(empty.is_empty());
  assert_eq!(empty.extend_from_slice(b"x"), Err(LimitError::CapacityExceeded));
  let mut full = SecretBytes::with_capacity(1).unwrap();
  full.extend_from_slice(b"x").unwrap();
  full.extend_from_slice(b"").unwrap();
  assert_eq!(full.expose(), b"x");
}

#[test]
fn impossible_capacity_is_a_recoverable_error() {
  assert!(matches!(SecretBytes::with_capacity(usize::MAX), Err(LimitError::AllocationFailed)));
}

#[test]
fn secret_view_borrows_the_original_bytes() {
  let bytes = b"borrowed secret";
  let exposed = {
    let view = SecretRef::new(bytes);
    view.expose()
  };
  assert_eq!(exposed, bytes);
  assert_eq!(exposed.as_ptr(), bytes.as_ptr());
}

#[test]
fn diagnostics_contain_only_metadata() {
  let mut secret = SecretBytes::with_capacity(12).unwrap();
  secret.extend_from_slice(b"private text").unwrap();
  assert_eq!(format!("{secret:?}"), "SecretBytes { len: 12, capacity: 12, .. }");
  assert_eq!(format!("{:?}", SecretRef::new(secret.expose())), "SecretRef { len: 12, .. }");
  assert_eq!(format!("{:?}", PayloadRef::Public(secret.expose())), "Public { len: 12, .. }");
  assert_eq!(
    format!("{:?}", PayloadRef::Secret(SecretRef::new(secret.expose()))),
    "Secret { len: 12, .. }"
  );
  let error = secret.extend_from_slice(b"private text").unwrap_err();
  assert_eq!(error.to_string(), "buffer capacity exceeded");
  assert_eq!(format!("{error:?}"), "CapacityExceeded");
}

#[test]
fn binary_bytes_are_preserved_without_text_conversion() {
  let mut secret = SecretBytes::with_capacity(4).unwrap();
  secret.extend_from_slice(&[0, 0xFF, b'\r', b'\n']).unwrap();
  match PayloadRef::Secret(SecretRef::new(secret.expose())) {
    PayloadRef::Secret(view) => assert_eq!(view.expose(), &[0, 0xFF, b'\r', b'\n']),
    PayloadRef::Public(_) => panic!("classification changed"),
  }
  assert_ne!(Sensitivity::Public, Sensitivity::Secret);
}
