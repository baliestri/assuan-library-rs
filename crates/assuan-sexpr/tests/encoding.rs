//! Canonical writer limits, atomic failure and owned-value behavior.

use assuan_sexpr::{EncodeError, OwnedSexpr, ParseLimits, Sexpr, parse_complete};

#[test]
fn owned_values_outlive_their_input() {
    let owned = {
        let input = Vec::from(b"((3:key)0:())");
        parse_complete(&input, ParseLimits::default())
            .unwrap()
            .to_owned()
    };
    assert_eq!(
        owned,
        OwnedSexpr::List(vec![
            OwnedSexpr::List(vec![OwnedSexpr::Atom(Box::from(&b"key"[..]))]),
            OwnedSexpr::Atom(Box::from(&b""[..])),
            OwnedSexpr::List(vec![]),
        ])
    );
    assert!(!format!("{owned:?}").contains("key"));
}

#[test]
fn writer_preserves_binary_atoms_and_adds_no_whitespace() {
    let value = Sexpr::List(vec![
        Sexpr::Atom(b"(\0)\xff"),
        Sexpr::List(vec![]),
        Sexpr::Atom(b" "),
    ]);
    assert_eq!(value.to_canonical().unwrap(), b"(4:(\0)\xff()1: )");
}

#[test]
fn writer_preserves_an_existing_prefix() {
    let mut output = b"prefix:".to_vec();
    Sexpr::Atom(b"").write_canonical(&mut output).unwrap();
    Sexpr::List(vec![]).write_canonical(&mut output).unwrap();
    assert_eq!(output, b"prefix:0:()");
}

#[test]
fn length_prefixes_cover_decimal_boundaries() {
    for length in [0, 1, 9, 10, 99, 100, 999, 1000] {
        let atom = vec![0xff; length];
        let wire = Sexpr::Atom(&atom).to_canonical().unwrap();
        let prefix = format!("{length}:");
        assert!(wire.starts_with(prefix.as_bytes()));
        assert_eq!(&wire[prefix.len()..], atom);
    }
}

#[test]
fn excessive_nesting_is_rejected_before_appending_any_bytes() {
    let mut deep = Sexpr::Atom(b"x");
    for _ in 0..65 {
        deep = Sexpr::List(vec![deep]);
    }
    let value = Sexpr::List(vec![Sexpr::Atom(b"valid-first"), deep]);
    let mut output = b"keep".to_vec();
    assert_eq!(
        value.write_canonical(&mut output),
        Err(EncodeError::DepthLimit)
    );
    assert_eq!(output, b"keep");
}

#[test]
fn wide_manually_constructed_lists_obey_node_budget() {
    let value = Sexpr::List((0..65_536).map(|_| Sexpr::Atom(b"")).collect());
    assert_eq!(value.to_canonical(), Err(EncodeError::NodeLimit));
}

#[test]
fn existing_output_counts_toward_the_byte_budget() {
    let mut output = vec![b'x'; ParseLimits::default().max_input()];
    let old_len = output.len();
    assert_eq!(
        Sexpr::Atom(b"").write_canonical(&mut output),
        Err(EncodeError::SizeLimit)
    );
    assert_eq!(output.len(), old_len);
    assert!(output.iter().all(|&byte| byte == b'x'));
}

#[test]
fn output_limit_counts_length_prefixes() {
    let atom = vec![b'a'; ParseLimits::default().max_atom()];
    assert_eq!(
        Sexpr::Atom(&atom).to_canonical(),
        Err(EncodeError::SizeLimit)
    );
}

#[test]
fn oversized_atoms_in_manual_trees_are_rejected() {
    let atom = vec![0; ParseLimits::default().max_atom() + 1];
    let mut output = b"keep".to_vec();
    assert_eq!(
        Sexpr::Atom(&atom).write_canonical(&mut output),
        Err(EncodeError::AtomLimit)
    );
    assert_eq!(output, b"keep");
}
