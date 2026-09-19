//! Binary payload, syntax, borrowing and resource-limit contracts.

use assuan_sexpr::{
    MAX_NESTING_DEPTH, ParseErrorKind, ParseLimits, Sexpr, parse_complete, parse_prefix,
};

#[test]
fn atoms_borrow_the_exact_input_region() {
    let input = Vec::from(b"4:data");
    let Sexpr::Atom(atom) = parse_complete(&input, ParseLimits::default()).unwrap() else {
        panic!("expected atom");
    };
    assert_eq!(atom.as_ptr(), input[2..].as_ptr());
    assert_eq!(atom, b"data");
}

#[test]
fn atom_payloads_are_binary_not_expression_syntax() {
    let tree = parse_complete(b"(3:\0\xff)0:)", ParseLimits::default()).unwrap();
    assert_eq!(
        tree,
        Sexpr::List(vec![Sexpr::Atom(b"\0\xff)"), Sexpr::Atom(b"")])
    );
}

#[test]
fn empty_atoms_and_lists_are_valid() {
    assert_eq!(
        parse_complete(b"0:", ParseLimits::default()).unwrap(),
        Sexpr::Atom(b"")
    );
    assert_eq!(
        parse_complete(b"()", ParseLimits::default()).unwrap(),
        Sexpr::List(vec![])
    );
}

#[test]
fn prefix_reports_consumption_without_parsing_the_suffix() {
    let input = b"(1:a)1:b";
    let (first, n) = parse_prefix(input, ParseLimits::default()).unwrap();
    assert_eq!(n, 5);
    assert_eq!(first, Sexpr::List(vec![Sexpr::Atom(b"a")]));
    assert_eq!(
        parse_complete(&input[n..], ParseLimits::default()).unwrap(),
        Sexpr::Atom(b"b")
    );
    assert_eq!(
        parse_prefix(b"0:garbage", ParseLimits::default())
            .unwrap()
            .1,
        2
    );
}

#[test]
fn complete_rejects_trailing_bytes_including_whitespace() {
    for input in [b"0: ".as_slice(), b"0:0:", b"0:)", b"0:\n"] {
        let error = parse_complete(input, ParseLimits::default()).unwrap_err();
        assert_eq!(error.kind(), ParseErrorKind::TrailingData);
        assert_eq!(error.offset(), Some(2));
    }
}

#[test]
fn empty_and_truncated_input_report_eof_without_panicking() {
    for input in [
        b"".as_slice(),
        b"(",
        b"(0:",
        b"3:ab",
        b"1",
        b"(1:a",
        b"((1:a)",
    ] {
        let error = parse_complete(input, ParseLimits::default()).unwrap_err();
        assert_eq!(error.kind(), ParseErrorKind::UnexpectedEof, "{input:?}");
        assert_eq!(error.offset(), Some(input.len()));
    }
}

#[test]
fn whitespace_outside_atoms_is_not_skipped() {
    for input in [b" ".as_slice(), b"\t", b"\r\n", b" 0:", b"\t()"] {
        let error = parse_complete(input, ParseLimits::default()).unwrap_err();
        assert_eq!(error.kind(), ParseErrorKind::UnexpectedToken);
        assert_eq!(error.offset(), Some(0));
    }
    let error = parse_complete(b"( )", ParseLimits::default()).unwrap_err();
    assert_eq!(error.kind(), ParseErrorKind::UnexpectedToken);
    assert_eq!(error.offset(), Some(1));
}

#[test]
fn atom_length_requires_canonical_digits_and_a_colon() {
    for input in [b"01:a".as_slice(), b"00:", b"0001:a"] {
        let error = parse_complete(input, ParseLimits::default()).unwrap_err();
        assert_eq!(error.kind(), ParseErrorKind::InvalidLength);
        assert_eq!(error.offset(), Some(1));
    }
    let error = parse_complete(b"1;a", ParseLimits::default()).unwrap_err();
    assert_eq!(error.kind(), ParseErrorKind::ExpectedColon);
    assert_eq!(error.offset(), Some(1));
    for input in [b":a".as_slice(), b"-1:a", b"+1:a", b")"] {
        assert_eq!(
            parse_complete(input, ParseLimits::default())
                .unwrap_err()
                .kind(),
            ParseErrorKind::UnexpectedToken
        );
    }
}

#[test]
fn atom_length_overflow_is_reported_before_payload_access() {
    let input = format!("{}0:", usize::MAX);
    let error = parse_complete(input.as_bytes(), ParseLimits::default()).unwrap_err();
    assert_eq!(error.kind(), ParseErrorKind::LengthOverflow);
}

#[test]
fn declared_length_is_limited_before_checking_for_missing_payload() {
    let limits = ParseLimits::new(32, 3, 8, 8).unwrap();
    assert_eq!(
        parse_complete(b"3:abc", limits).unwrap(),
        Sexpr::Atom(b"abc")
    );
    let error = parse_complete(b"4:", limits).unwrap_err();
    assert_eq!(error.kind(), ParseErrorKind::AtomLimit);
    assert_eq!(error.offset(), Some(0));
}

#[test]
fn input_limit_includes_an_unconsumed_suffix() {
    let limits = ParseLimits::new(4, 2, 4, 4).unwrap();
    assert_eq!(parse_complete(b"2:ab", limits).unwrap(), Sexpr::Atom(b"ab"));
    let error = parse_prefix(b"0:abc", limits).unwrap_err();
    assert_eq!(error.kind(), ParseErrorKind::InputLimit);
    assert_eq!(error.offset(), Some(4));
}

#[test]
fn node_budget_counts_lists_and_atoms_once() {
    let limits = ParseLimits::new(64, 64, 3, 8).unwrap();
    assert!(parse_complete(b"(1:a1:b)", limits).is_ok());
    assert!(parse_complete(b"(()())", limits).is_ok());
    for input in [b"(1:a1:b0:)".as_slice(), b"((0:)())"] {
        assert_eq!(
            parse_complete(input, limits).unwrap_err().kind(),
            ParseErrorKind::NodeLimit
        );
    }
}

#[test]
fn default_depth_accepts_64_and_rejects_65_lists() {
    for (depth, accepted) in [(64, true), (65, false)] {
        let input = format!("{}0:{}", "(".repeat(depth), ")".repeat(depth));
        let result = parse_complete(input.as_bytes(), ParseLimits::default());
        if accepted {
            assert!(result.is_ok());
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.kind(), ParseErrorKind::DepthLimit);
            assert_eq!(error.offset(), Some(64));
        }
    }
}

#[test]
fn extreme_unclosed_nesting_is_rejected_at_the_depth_limit() {
    let input = vec![b'('; 100_000];
    let error = parse_complete(&input, ParseLimits::default()).unwrap_err();
    assert_eq!(error.kind(), ParseErrorKind::DepthLimit);
    assert_eq!(error.offset(), Some(64));
}

#[test]
fn configured_maximum_depth_produces_a_tree_that_can_be_dropped() {
    let input = format!(
        "{}0:{}",
        "(".repeat(MAX_NESTING_DEPTH),
        ")".repeat(MAX_NESTING_DEPTH)
    );
    let limits = ParseLimits::new(1024, 1024, 1024, MAX_NESTING_DEPTH).unwrap();
    let value = parse_complete(input.as_bytes(), limits).unwrap();
    drop(value);
    assert!(ParseLimits::new(1024, 1024, 1024, MAX_NESTING_DEPTH + 1).is_err());
}

#[test]
fn configuration_errors_do_not_invent_input_offsets() {
    for parameters in [(0, 0, 1, 1), (2, 3, 1, 1), (2, 2, 0, 1)] {
        let error =
            ParseLimits::new(parameters.0, parameters.1, parameters.2, parameters.3).unwrap_err();
        assert_eq!(error.kind(), ParseErrorKind::InvalidLimits);
        assert_eq!(error.offset(), None);
    }
    let limits = ParseLimits::new(4, 0, 1, 0).unwrap();
    assert!(parse_complete(b"0:", limits).is_ok());
    assert_eq!(
        parse_complete(b"()", limits).unwrap_err().kind(),
        ParseErrorKind::DepthLimit
    );
}

#[test]
fn diagnostics_do_not_expose_atom_contents() {
    let tree = parse_complete(b"(9:superkey!)", ParseLimits::default()).unwrap();
    assert!(!format!("{tree:?}").contains("superkey"));
    let error = parse_complete(b"(9:superkey!x)", ParseLimits::default()).unwrap_err();
    assert!(!format!("{error:?}").contains("superkey"));
    assert!(!error.to_string().contains("superkey"));
}
