//! Generated binary-atom and nested-tree canonical round trips.

use assuan_sexpr::{ParseLimits, Sexpr, parse_complete};
use proptest::prelude::*;

fn canonical_wire() -> impl Strategy<Value = Vec<u8>> {
    let atom = proptest::collection::vec(any::<u8>(), 0..64).prop_map(|bytes| {
        let mut wire = format!("{}:", bytes.len()).into_bytes();
        wire.extend(bytes);
        wire
    });
    atom.prop_recursive(5, 96, 4, |inner| {
        proptest::collection::vec(inner, 0..5).prop_map(|children| {
            let mut wire = vec![b'('];
            wire.extend(children.into_iter().flatten());
            wire.push(b')');
            wire
        })
    })
}

proptest! {
    #[test]
    fn arbitrary_binary_atoms_roundtrip(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let wire = Sexpr::Atom(&bytes).to_canonical().unwrap();
        let decoded = parse_complete(&wire, ParseLimits::default()).unwrap();
        prop_assert_eq!(decoded, Sexpr::Atom(&bytes));
    }

    #[test]
    fn canonical_trees_reencode_exactly(wire in canonical_wire()) {
        let value = parse_complete(&wire, ParseLimits::default()).unwrap();
        prop_assert_eq!(value.to_canonical().unwrap(), wire);
    }
}
