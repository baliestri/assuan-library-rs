//! Regression tests for canonical expression tree structure.

use assuan_sexpr::{ParseLimits, Sexpr, parse_complete};

#[test]
fn nested_siblings_keep_their_parent() {
  let tree = parse_complete(b"((1:a)(1:b))", ParseLimits::default()).unwrap();
  assert_eq!(
    tree,
    Sexpr::List(vec![Sexpr::List(vec![Sexpr::Atom(b"a")]), Sexpr::List(vec![Sexpr::Atom(b"b")]),])
  );
}
