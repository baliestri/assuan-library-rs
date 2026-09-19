//! Complete-token parsing, borrowing, numeric bounds, and atomic decoding.

use assuan_protocol::{
  Command, ProtocolError, ServerLine, decode_data_in_place, parse_server_line,
};

#[test]
fn custom_command_arguments_are_borrowed_and_unmodified() {
  let line = b"custom-command  %GG %ff \xff\t ";
  let command = Command::parse(line).unwrap();
  assert_eq!(command.name(), "custom-command");
  assert_eq!(command.args(), b" %GG %ff \xff\t ");
  assert_eq!(command.name().as_ptr(), line.as_ptr());
  assert_eq!(command.args().as_ptr(), line[15..].as_ptr());
  let built = Command::new("custom-command", command.args()).unwrap();
  assert_eq!(built.args().as_ptr(), command.args().as_ptr());
}

#[test]
fn command_names_have_no_implicit_normalization() {
  for name in ["GETINFO", "getinfo", "X_CUSTOM-1"] {
    assert_eq!(Command::new(name, b"").unwrap().name(), name);
  }
  for name in ["", " GETINFO", "GET INFO", "GET\tINFO", "%47ET", "#comment", "ação", "X\0"] {
    assert!(Command::new(name, b"").is_err(), "accepted {name:?}");
    assert!(Command::parse(name.as_bytes()).is_err() || name == "GET INFO");
  }
  assert!(Command::parse(b"NOP").unwrap().args().is_empty());
  assert!(Command::parse(b"NOP ").unwrap().args().is_empty());
}

#[test]
fn line_injection_is_rejected_by_both_command_entry_points() {
  for args in [b"x\rNOP".as_slice(), b"x\nNOP", b"x\0NOP"] {
    assert!(matches!(Command::new("CUSTOM", args), Err(ProtocolError::InvalidLine)));
    let mut line = b"CUSTOM ".to_vec();
    line.extend_from_slice(args);
    assert!(matches!(Command::parse(&line), Err(ProtocolError::InvalidLine)));
  }
}

#[test]
fn responses_require_complete_tokens_and_exact_separators() {
  for line in [
    b"OKAY".as_slice(),
    b"Dfoo",
    b"D",
    b"ERR",
    b"ERRx 1",
    b"OK\ttext",
    b"END extra",
    b"END ",
    b"Sfoo",
    b"INQUIREfoo",
    b" OK",
    b"ok",
  ] {
    assert!(parse_server_line(line).is_err(), "accepted {line:?}");
  }
  assert!(matches!(parse_server_line(b"OK"), Ok(ServerLine::Ok(b""))));
  assert!(matches!(parse_server_line(b"OK  \xff "), Ok(ServerLine::Ok(b" \xff "))));
  assert!(matches!(parse_server_line(b"END"), Ok(ServerLine::End)));
  assert!(matches!(parse_server_line(b""), Ok(ServerLine::Empty)));
  assert!(matches!(parse_server_line(b"#  comment"), Ok(ServerLine::Comment(b"  comment"))));
}

#[test]
fn error_codes_preserve_all_source_bits_and_check_overflow() {
  for (line, expected) in
    [(b"ERR 0".as_slice(), 0), (b"ERR 4294967295", u32::MAX), (b"ERR 00042 text", 42)]
  {
    let ServerLine::Err {
      code,
      ..
    } = parse_server_line(line).unwrap()
    else {
      panic!("wrong variant")
    };
    assert_eq!(code, expected);
  }
  for line in [
    b"ERR ".as_slice(),
    b"ERR x",
    b"ERR -1",
    b"ERR +1",
    b"ERR 4294967296",
    b"ERR 99999999999999999999999",
    b"ERR  1",
    b"ERR 1\ttext",
  ] {
    assert!(matches!(parse_server_line(line), Err(ProtocolError::InvalidErrorCode)));
  }
  assert!(matches!(
    parse_server_line(b"ERR 42  message "),
    Ok(ServerLine::Err {
      code: 42,
      text: b" message "
    })
  ));
}

#[test]
fn unknown_keywords_and_binary_arguments_are_preserved() {
  let ServerLine::Status {
    keyword,
    args,
  } = parse_server_line(b"S _CUSTOM-9  \xff%GG ").unwrap()
  else {
    panic!("wrong variant")
  };
  assert_eq!(keyword, "_CUSTOM-9");
  assert_eq!(args, b" \xff%GG ");
  assert!(matches!(
    parse_server_line(b"INQUIRE foo"),
    Ok(ServerLine::Inquire {
      keyword: "foo",
      args: b""
    })
  ));
  for line in [
    b"S ".as_slice(),
    b"S  foo",
    b"S 9FOO",
    b"S %41",
    b"S \xff",
    b"INQUIRE ",
    b"INQUIRE  foo",
    b"INQUIRE 9foo",
  ] {
    assert!(matches!(parse_server_line(line), Err(ProtocolError::InvalidToken)));
  }
}

#[test]
fn data_is_borrowed_without_implicit_decoding() {
  let line = b"D  %ff%GG ";
  let ServerLine::Data(data) = parse_server_line(line).unwrap() else {
    panic!("wrong variant")
  };
  assert_eq!(data, b" %ff%GG ");
  assert_eq!(data.as_ptr(), line[2..].as_ptr());
  assert!(matches!(parse_server_line(b"D "), Ok(ServerLine::Data(b""))));
}

#[test]
fn embedded_terminators_and_nul_are_rejected_in_server_lines() {
  for line in [b"OK\n".as_slice(), b"D x\r", b"# x\0", b"S STATUS\r\n"] {
    assert!(matches!(parse_server_line(line), Err(ProtocolError::InvalidLine)));
  }
}

#[test]
fn invalid_escapes_never_partially_decode_the_input() {
  for input in [b"%".as_slice(), b"%0", b"%GG", b"%0x", b"%x0", b"%41 then %GG", b"%41%"] {
    let mut bytes = input.to_vec();
    assert_eq!(decode_data_in_place(&mut bytes), Err(ProtocolError::InvalidEscape));
    assert_eq!(bytes, input);
  }
}

#[test]
fn all_bytes_decode_from_both_hex_cases() {
  for original in 0_u8..=255 {
    for encoded in [format!("%{original:02X}"), format!("%{original:02x}")] {
      let mut bytes = encoded.into_bytes();
      assert_eq!(decode_data_in_place(&mut bytes), Ok(1));
      assert_eq!(bytes[0], original);
    }
  }
  for input in [b"".as_slice(), b"   ", b"\0\xff\t"] {
    let mut bytes = input.to_vec();
    assert_eq!(decode_data_in_place(&mut bytes), Ok(input.len()));
    assert_eq!(bytes, input);
  }
  let mut bytes = *b"%2541";
  let len = decode_data_in_place(&mut bytes).unwrap();
  assert_eq!(&bytes[..len], b"%41");
}

#[test]
fn debug_output_never_contains_payload_or_keyword_contents() {
  for line in [
    b"OK sensitive".as_slice(),
    b"ERR 1 sensitive",
    b"D sensitive",
    b"S sensitive sensitive",
    b"INQUIRE sensitive sensitive",
    b"# sensitive",
  ] {
    assert!(!format!("{:?}", parse_server_line(line).unwrap()).contains("sensitive"));
  }
  assert!(!format!("{:?}", Command::new("sensitive", b"sensitive").unwrap()).contains("sensitive"));
}
