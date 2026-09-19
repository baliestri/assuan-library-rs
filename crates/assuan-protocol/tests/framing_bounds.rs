//! Boundary, segmentation, and encoder atomicity regressions.

use assuan_protocol::{
  Command, LineBuffer, MAX_LINE_BYTES, ProtocolError, ServerLine, decode_data_in_place,
  encode_command, encode_data_chunk, encode_response, parse_server_line,
};

#[test]
fn lf_and_crlf_count_toward_the_wire_limit() {
  for ending in [b"\n".as_slice(), b"\r\n"] {
    for total in [999, 1000, 1001] {
      let mut wire = vec![b'x'; total - ending.len()];
      wire.extend_from_slice(ending);
      let mut frame = LineBuffer::new();
      if total <= MAX_LINE_BYTES {
        assert_eq!(frame.feed(&wire), Ok(total));
        assert_eq!(frame.line().unwrap().len(), total - ending.len());
        assert!(frame.finish_eof().is_ok());
      } else {
        assert_eq!(frame.feed(&wire), Err(ProtocolError::LineTooLong));
        assert!(frame.line().is_none());
        assert_eq!(frame.feed(b"\n"), Err(ProtocolError::LineTooLong));
        assert_eq!(frame.finish_eof(), Err(ProtocolError::LineTooLong));
      }
    }
  }
}

#[test]
fn eof_and_ready_line_behave_independently() {
  let mut frame = LineBuffer::default();
  assert_eq!(frame.finish_eof(), Ok(()));
  assert_eq!(frame.feed(b"OK\r"), Ok(3));
  assert!(frame.line().is_none());
  assert_eq!(frame.finish_eof(), Err(ProtocolError::UnexpectedEof));
  assert_eq!(frame.feed(b"\nignored"), Ok(1));
  assert_eq!(frame.feed(b"next\n"), Ok(0));
  assert_eq!(frame.line(), Some(b"OK".as_slice()));
  frame.clear();
  assert_eq!(frame.feed(b"\n"), Ok(1));
  assert_eq!(frame.line(), Some(b"".as_slice()));
  frame.clear();
  assert_eq!(frame.feed(b""), Ok(0));
  assert!(frame.line().is_none());
}

#[test]
fn every_partition_preserves_both_frames() {
  let wire = b"D a%0Ab\r\nOK\n";
  for mask in 0_usize..(1 << (wire.len() - 1)) {
    let mut frame = LineBuffer::new();
    let mut lines = Vec::new();
    let mut start = 0;
    for end in 1..=wire.len() {
      if end != wire.len() && mask & (1 << (end - 1)) == 0 {
        continue;
      }
      let mut chunk = &wire[start..end];
      while !chunk.is_empty() {
        let consumed = frame.feed(chunk).unwrap();
        assert!(consumed > 0);
        chunk = &chunk[consumed..];
        if let Some(line) = frame.line() {
          lines.push(line.to_vec());
          frame.clear();
        }
      }
      start = end;
    }
    assert_eq!(lines, [b"D a%0Ab".to_vec(), b"OK".to_vec()]);
    assert_eq!(frame.finish_eof(), Ok(()));
  }
}

#[test]
fn command_encoding_is_bounded_and_atomic() {
  let mut output = [0xAA; MAX_LINE_BYTES];
  let len = encode_command(Command::new("NOP", b"").unwrap(), &mut output).unwrap();
  assert_eq!(&output[..len], b"NOP\n");
  assert!(output[len..].iter().all(|byte| return *byte == 0xAA));
  let args = [b'x'; 997];
  assert_eq!(encode_command(Command::new("X", &args).unwrap(), &mut output), Ok(1000));
  let before = output;
  assert_eq!(
    encode_command(Command::new("XX", &args).unwrap(), &mut output),
    Err(ProtocolError::LineTooLong)
  );
  assert_eq!(output, before);
  let len = encode_command(Command::new("CUSTOM", b" %GG \xff").unwrap(), &mut output).unwrap();
  assert_eq!(&output[..len], b"CUSTOM  %GG \xff\n");
}

#[test]
fn data_chunks_handle_empty_binary_and_escape_boundaries() {
  let mut output = [0; MAX_LINE_BYTES];
  assert_eq!(encode_data_chunk(b"", &mut output), Ok((0, 3)));
  assert_eq!(&output[..3], b"D \n");
  let (used, written) = encode_data_chunk(b"%\r\n\0\xff ", &mut output).unwrap();
  assert_eq!(used, 6);
  assert_eq!(&output[..written], b"D %25%0D%0A%00\xff \n");
  for plain in [994, 995, 996, 997] {
    let mut input = vec![b'a'; plain];
    input.push(b'%');
    let (used, written) = encode_data_chunk(&input, &mut output).unwrap();
    assert!(used > 0);
    assert!(written <= MAX_LINE_BYTES);
    assert_eq!(
      used,
      if plain == 994 {
        995
      } else {
        plain
      }
    );
    assert_eq!(output[written - 1], b'\n');
    let len = decode_data_in_place(&mut output[2..written - 1]).unwrap();
    assert_eq!(&output[2..2 + len], &input[..used]);
  }
}

#[test]
fn arbitrary_binary_data_roundtrips_across_many_chunks() {
  let input: Vec<u8> = (0_u8..=255).cycle().take(8192).collect();
  let mut remaining = input.as_slice();
  let mut decoded = Vec::new();
  let mut output = [0; MAX_LINE_BYTES];
  while !remaining.is_empty() {
    let (used, written) = encode_data_chunk(remaining, &mut output).unwrap();
    assert!(used > 0);
    let mut frame = LineBuffer::new();
    assert_eq!(frame.feed(&output[..written]), Ok(written));
    let ServerLine::Data(data) = parse_server_line(frame.line().unwrap()).unwrap() else {
      panic!("expected data")
    };
    let mut data = data.to_vec();
    let len = decode_data_in_place(&mut data).unwrap();
    decoded.extend_from_slice(&data[..len]);
    remaining = &remaining[used..];
  }
  assert_eq!(decoded, input);
}

#[test]
fn response_categories_encode_without_altering_fields() {
  let cases = [
    (ServerLine::Ok(b"done"), b"OK done\n".as_slice()),
    (ServerLine::Ok(b""), b"OK\n"),
    (
      ServerLine::Err {
        code: u32::MAX,
        text: b" failure",
      },
      b"ERR 4294967295  failure\n",
    ),
    (
      ServerLine::Err {
        code: 0,
        text: b"",
      },
      b"ERR 0\n",
    ),
    (ServerLine::Data(b" %ff"), b"D  %ff\n"),
    (
      ServerLine::Status {
        keyword: "_STATUS",
        args: b" x",
      },
      b"S _STATUS  x\n",
    ),
    (
      ServerLine::Inquire {
        keyword: "foo",
        args: b"",
      },
      b"INQUIRE foo\n",
    ),
    (ServerLine::End, b"END\n"),
    (ServerLine::Comment(b" note"), b"# note\n"),
    (ServerLine::Empty, b"\n"),
  ];
  for (line, expected) in cases {
    let mut output = [0xAA; MAX_LINE_BYTES];
    let len = encode_response(line, &mut output).unwrap();
    assert_eq!(&output[..len], expected);
    assert!(output[len..].iter().all(|byte| return *byte == 0xAA));
    assert!(parse_server_line(&output[..len - 1]).is_ok());
  }
}

#[test]
fn invalid_responses_leave_output_unchanged() {
  let large = [b'x'; 1000];
  for line in [
    ServerLine::Ok(b"injected\nERR 1"),
    ServerLine::Err {
      code: 1,
      text: b"\0",
    },
    ServerLine::Data(b"%G0"),
    ServerLine::Data(b"%0"),
    ServerLine::Data(b"\r"),
    ServerLine::Status {
      keyword: "",
      args: b"",
    },
    ServerLine::Status {
      keyword: "9bad",
      args: b"",
    },
    ServerLine::Inquire {
      keyword: "bad keyword",
      args: b"",
    },
    ServerLine::Inquire {
      keyword: "valid",
      args: b"\n",
    },
    ServerLine::Comment(b"\r"),
    ServerLine::Ok(&large),
  ] {
    let mut output = [0xAA; MAX_LINE_BYTES];
    assert!(encode_response(line, &mut output).is_err());
    assert_eq!(output, [0xAA; MAX_LINE_BYTES]);
  }
}

#[test]
fn frame_debug_does_not_expose_buffered_bytes() {
  let mut frame = LineBuffer::new();
  frame.feed(b"secret\n").unwrap();
  assert!(!format!("{frame:?}").contains("secret"));
}
