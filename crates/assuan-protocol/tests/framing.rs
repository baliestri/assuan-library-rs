//! Incremental framing respects message boundaries.

#[test]
fn framing_does_not_swallow_the_next_line() {
  let mut frames = assuan_protocol::LineBuffer::new();
  let used = frames.feed(b"OK\r\nD x\n").unwrap();
  assert_eq!(used, 4);
  assert_eq!(frames.line(), Some(b"OK".as_slice()));
}
