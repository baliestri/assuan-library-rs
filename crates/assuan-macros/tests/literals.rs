//! Literal expansion through the public procedural macros.

#[test]
fn literals_preserve_static_bytes() {
  let command: assuan_protocol::Command<'static> = assuan_macros::command!("GETINFO version");
  assert_eq!(command.name(), "GETINFO");
  assert_eq!(command.args(), b"version");
  let value: &'static [u8] = assuan_macros::sexpr!(("hash" "sha256"));
  assert_eq!(value, b"(4:hash6:sha256)");
}

#[test]
fn command_preserves_case_spaces_unicode_and_wire_escapes() {
  let command = assuan_macros::command!(r"GetInfo  café%20 ");
  assert_eq!(command.name(), "GetInfo");
  assert_eq!(command.args(), " café%20 ".as_bytes());
  let bare = assuan_macros::command!("NOP");
  assert_eq!(bare.args(), b"");
}

#[test]
fn sexpr_is_a_const_static_slice_with_binary_and_nested_atoms() {
  const VALUE: &[u8] = assuan_macros::sexpr!(("hash" ("sha256" b"\x00\xff") ""));
  static EMPTY: &[u8] = assuan_macros::sexpr!(());
  assert_eq!(VALUE, b"(4:hash(6:sha2562:\x00\xff)0:)");
  assert_eq!(EMPTY, b"()");
  let parsed = assuan_sexpr::parse_complete(VALUE, assuan_sexpr::ParseLimits::default()).unwrap();
  assert_eq!(parsed.to_canonical().unwrap(), VALUE);
}

#[test]
fn atom_lengths_count_utf8_bytes_and_preserve_delimiter_bytes() {
  let value = assuan_macros::sexpr!(("é" "🦀" br"(:)" b"\r\n\0"));
  assert_eq!(value, b"(2:\xc3\xa94:\xf0\x9f\xa6\x803:(:)3:\r\n\0)");
  assert_eq!(assuan_macros::sexpr!(r"raw\text"), br"8:raw\text");
}

#[test]
fn top_level_atoms_and_empty_atoms_are_supported() {
  assert_eq!(assuan_macros::sexpr!(""), b"0:");
  assert_eq!(assuan_macros::sexpr!(b"\xff"), b"1:\xff");
}
