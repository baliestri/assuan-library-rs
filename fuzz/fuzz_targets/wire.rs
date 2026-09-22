#![no_main]

use assuan_protocol::{Command, LineBuffer, decode_data_in_place, parse_server_line};
use libfuzzer_sys::fuzz_target;

// Compare complete frames, framing failure, and EOF validity across chunking.
fn framed<'a>(chunks: impl IntoIterator<Item = &'a [u8]>) -> (Vec<Vec<u8>>, bool, bool) {
  let mut buffer = LineBuffer::new();
  let mut lines = Vec::new();
  for mut chunk in chunks {
    while !chunk.is_empty() {
      let Ok(used) = buffer.feed(chunk) else {
        assert!(buffer.feed(b"\n").is_err());
        return (lines, true, buffer.finish_eof().is_ok());
      };
      assert!(used > 0 && used <= chunk.len());
      chunk = &chunk[used..];
      if let Some(line) = buffer.line() {
        let _ = Command::parse(line);
        let _ = parse_server_line(line);
        let mut decoded = line.to_vec();
        let _ = decode_data_in_place(&mut decoded);
        lines.push(line.to_vec());
        buffer.clear();
      }
    }
  }
  return (lines, false, buffer.finish_eof().is_ok());
}

fuzz_target!(|bytes: &[u8]| {
  let bytes = &bytes[..bytes.len().min(4096)];
  let expected = framed([bytes]);
  if bytes.len() <= 64 {
    for split in 0..=bytes.len() {
      assert_eq!(expected, framed([&bytes[..split], &bytes[split..]]));
    }
  } else {
    for selector in bytes.iter().take(8) {
      let split = usize::from(*selector) * bytes.len() / 255;
      assert_eq!(expected, framed([&bytes[..split], &bytes[split..]]));
    }
  }
  let width = bytes.first().map_or(1, |byte| return usize::from(*byte) + 1);
  assert_eq!(expected, framed(bytes.chunks(width)));
  return;
});
