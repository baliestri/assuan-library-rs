//! Fixed-capacity secret storage regression tests.

#[test]
fn secret_capacity_is_a_hard_limit() {
    let mut value = assuan_protocol::SecretBytes::with_capacity(4).unwrap();
    value.extend_from_slice(b"pass").unwrap();
    assert!(value.extend_from_slice(b"x").is_err());
    assert_eq!(value.expose(), b"pass");
    assert!(!format!("{value:?}").contains("pass"));
    value.clear();
    assert!(value.expose().is_empty());
}
