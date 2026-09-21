//! Total operation deadlines and cancellation cannot resurrect a session.
use std::time::Duration;

use assuan_client::{Client, ClientError, ClientOptions};
use assuan_protocol::Command;
use assuan_transport::{Stream, TransportError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test(start_paused = true)]
async fn greeting_timeout_and_invalid_options_drop_owned_streams() {
  let (io, mut peer) = tokio::io::duplex(128);
  let options = ClientOptions {
    greeting_timeout: Duration::from_secs(1),
    ..Default::default()
  };
  assert!(matches!(
    Client::from_stream(Stream::new(io), options).await,
    Err(ClientError::Transport(TransportError::Timeout))
  ));
  assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
  for timeout in [Duration::ZERO, Duration::MAX] {
    let (io, mut peer) = tokio::io::duplex(128);
    let options = ClientOptions {
      command_timeout: timeout,
      ..Default::default()
    };
    assert!(matches!(
      Client::from_stream(Stream::new(io), options).await,
      Err(ClientError::InvalidOptions)
    ));
    assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
  }
}

#[tokio::test(start_paused = true)]
async fn command_deadline_does_not_restart_after_data() {
  let (io, mut peer) = tokio::io::duplex(128);
  peer.write_all(b"OK\n").await.unwrap();
  let options = ClientOptions {
    command_timeout: Duration::from_secs(5),
    ..Default::default()
  };
  let mut client = Client::from_stream(Stream::new(io), options).await.unwrap();
  let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  peer.read_exact(&mut [0; 4]).await.unwrap();
  tokio::time::advance(Duration::from_secs(4)).await;
  peer.write_all(b"D hello\n").await.unwrap();
  tx.next().await.unwrap();
  tokio::time::advance(Duration::from_secs(1)).await;
  peer.write_all(b"OK\n").await.unwrap();
  assert!(matches!(tx.next().await, Err(ClientError::Transport(TransportError::Timeout))));
  drop(tx);
  assert!(!client.is_usable());
}

#[tokio::test]
async fn cancelling_a_pending_read_poisoned_the_live_transaction() {
  let (io, mut peer) = tokio::io::duplex(128);
  peer.write_all(b"OK\n").await.unwrap();
  let mut client = Client::from_stream(Stream::new(io), ClientOptions::default()).await.unwrap();
  let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  peer.read_exact(&mut [0; 4]).await.unwrap();
  peer.write_all(b"D partial").await.unwrap();
  tokio::select! {
    biased;
    result = tx.next() => panic!("unexpected: {result:?}"),
    () = std::future::ready(()) => {}
  }
  assert!(tx.next().await.is_err());
  assert!(tx.finish().is_err());
  assert!(!client.is_usable());
  assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
}

#[tokio::test]
async fn cancelling_a_partial_command_write_prevents_reuse() {
  let (io, mut peer) = tokio::io::duplex(3);
  peer.write_all(b"OK\n").await.unwrap();
  let mut client = Client::from_stream(Stream::new(io), ClientOptions::default()).await.unwrap();
  tokio::select! {
    biased;
    result = client.command(Command::new("NOP", b"").unwrap()) => panic!("unexpected: {result:?}"),
    () = std::future::ready(()) => {}
  }
  assert!(!client.is_usable());
  assert!(client.command(Command::new("NOP", b"").unwrap()).await.is_err());
  let mut bytes = Vec::new();
  peer.read_to_end(&mut bytes).await.unwrap();
  assert_eq!(bytes, b"NOP");
}
