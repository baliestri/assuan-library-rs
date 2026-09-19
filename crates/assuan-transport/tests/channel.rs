//! Async framing over independent in-memory byte streams.
use assuan_protocol::Sensitivity;
use assuan_transport::{Channel, Stream};
use std::time::Duration;
use tokio::{io::AsyncWriteExt, time::Instant};

#[tokio::test]
async fn channel_reassembles_fragmented_line() {
  let (client, mut peer) = tokio::io::duplex(16);
  let task = tokio::spawn(async move {
    peer.write_all(b"D a%").await.unwrap();
    tokio::task::yield_now().await;
    peer.write_all(b"0A\n").await.unwrap();
  });
  let mut channel = Channel::new(Stream::new(client));
  let deadline = Instant::now() + Duration::from_secs(1);
  assert_eq!(channel.read_line(deadline, Sensitivity::Public).await.unwrap(), b"D a%0A");
  task.await.unwrap();
}

fn deadline() -> Instant {
  return Instant::now() + Duration::from_secs(2);
}

#[tokio::test]
async fn coalesced_lines_allow_in_place_decoding_without_corrupting_read_ahead() {
  let (client, mut peer) = tokio::io::duplex(128);
  peer.write_all(b"D a%0A\r\nOK next\n").await.unwrap();
  let mut channel = Channel::new(Stream::new(client));
  let first = channel.read_line(deadline(), Sensitivity::Public).await.unwrap();
  first.fill(b'x');
  assert_eq!(channel.read_line(deadline(), Sensitivity::Public).await.unwrap(), b"OK next");
}

#[tokio::test]
async fn accepts_exact_limit_and_rejects_oversized_lines() {
  for length in [1000, 1001] {
    let (client, mut peer) = tokio::io::duplex(2048);
    let mut bytes = vec![b'x'; length];
    bytes[length - 1] = b'\n';
    peer.write_all(&bytes).await.unwrap();
    let mut channel = Channel::new(Stream::new(client));
    let result = channel.read_line(deadline(), Sensitivity::Public).await;
    if length == 1000 {
      assert_eq!(result.unwrap().len(), 999);
    } else {
      assert!(matches!(
        result,
        Err(assuan_transport::TransportError::Protocol(
          assuan_protocol::ProtocolError::LineTooLong
        ))
      ));
    }
  }
}

#[tokio::test]
async fn eof_never_completes_a_line() {
  for bytes in [b"".as_slice(), b"D incomplete"] {
    let (client, mut peer) = tokio::io::duplex(128);
    peer.write_all(bytes).await.unwrap();
    drop(peer);
    let mut channel = Channel::new(Stream::new(client));
    assert!(matches!(
      channel.read_line(deadline(), Sensitivity::Public).await,
      Err(assuan_transport::TransportError::Protocol(
        assuan_protocol::ProtocolError::UnexpectedEof
      ))
    ));
  }
}

#[tokio::test]
async fn write_all_handles_backpressure_and_preserves_the_wire_line() {
  use tokio::io::AsyncReadExt;
  let (client, mut peer) = tokio::io::duplex(2);
  let mut channel = Channel::new(Stream::new(client));
  let receive = tokio::spawn(async move {
    let mut bytes = [0; 10];
    peer.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"D a%0Axy\r\n");
  });
  channel.write_line(b"D a%0Axy\r\n", deadline(), Sensitivity::Public).await.unwrap();
  receive.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn pending_read_and_write_obey_total_deadlines() {
  let (client, _peer) = tokio::io::duplex(1);
  let mut channel = Channel::new(Stream::new(client));
  assert!(matches!(
    channel.read_line(deadline(), Sensitivity::Public).await,
    Err(assuan_transport::TransportError::Timeout)
  ));
  assert!(matches!(
    channel.write_line(b"NOP\n", deadline(), Sensitivity::Public).await,
    Err(assuan_transport::TransportError::Timeout)
  ));
}

#[tokio::test]
async fn invalid_wire_lines_are_rejected_before_delivery() {
  use tokio::io::AsyncReadExt;
  let (client, mut peer) = tokio::io::duplex(2048);
  let mut channel = Channel::new(Stream::new(client));
  for bytes in [b"".as_slice(), b"NOP", b"NOP\nBYE\n", b"D \0\n", b"D \rX\n", &[b'x'; 1001]] {
    assert!(matches!(
      channel.write_line(bytes, deadline(), Sensitivity::Public).await,
      Err(assuan_transport::TransportError::Protocol(_))
    ));
  }
  channel.close();
  let mut received = Vec::new();
  peer.read_to_end(&mut received).await.unwrap();
  assert!(received.is_empty());
  channel.close();
  assert!(matches!(
    channel.read_line(deadline(), Sensitivity::Public).await,
    Err(assuan_transport::TransportError::Closed)
  ));
  assert!(matches!(
    channel.write_line(b"NOP\n", deadline(), Sensitivity::Public).await,
    Err(assuan_transport::TransportError::Closed)
  ));
}

#[tokio::test]
async fn expired_deadline_prevents_immediately_ready_io() {
  let (client, mut peer) = tokio::io::duplex(128);
  peer.write_all(b"OK\n").await.unwrap();
  let mut channel = Channel::new(Stream::new(client));
  let expired = Instant::now();
  assert!(matches!(
    channel.read_line(expired, Sensitivity::Public).await,
    Err(assuan_transport::TransportError::Timeout)
  ));
  assert!(matches!(
    channel.write_line(b"NOP\n", expired, Sensitivity::Public).await,
    Err(assuan_transport::TransportError::Timeout)
  ));
}
