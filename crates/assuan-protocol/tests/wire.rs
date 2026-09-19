//! Wire parsing preserves spaces and arbitrary payload bytes.

#[test]
fn spaces_and_binary_data_survive() {
  let mut raw = *b"  %FF%0a";
  let n = assuan_protocol::decode_data_in_place(&mut raw).unwrap();
  assert_eq!(&raw[..n], b"  \xff\n");
}
