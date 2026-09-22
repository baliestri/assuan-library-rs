#![no_main]

use assuan_sexpr::{ParseLimits, parse_complete};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
  let limits = ParseLimits::new(4096, 4096, 1024, 64).unwrap();
  if let Ok(value) = parse_complete(bytes, limits) {
    let encoded = value.to_canonical().unwrap();
    let again = parse_complete(&encoded, limits).unwrap();
    assert_eq!(value, again);
    assert_eq!(encoded, again.to_canonical().unwrap());
  }
  return;
});
