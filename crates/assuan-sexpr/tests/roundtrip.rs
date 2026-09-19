//! Canonical encoding and explicit ownership contracts.

use assuan_sexpr::Sexpr;

#[test]
fn canonical_length_counts_bytes() {
    let value = Sexpr::Atom("é".as_bytes());
    assert_eq!(value.to_canonical().unwrap(), b"2:\xc3\xa9");
}
