//! Cancelling only an I/O future cannot restore a live session.
use assuan_client::{Client, ClientOptions};
use assuan_protocol::Command;
use assuan_transport::Stream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test(start_paused = true)]
async fn dropping_pending_read_poisoned_the_session() {
  let (io, mut peer) = tokio::io::duplex(64);
  peer.write_all(b"OK\n").await.unwrap();
  let mut client = Client::from_stream(Stream::new(io), ClientOptions::default()).await.unwrap();
  let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  let mut sent = [0; 4];
  peer.read_exact(&mut sent).await.unwrap();
  assert_eq!(&sent, b"NOP\n");
  assert!(tokio::time::timeout(std::time::Duration::from_millis(1), tx.next()).await.is_err());
  assert!(tx.next().await.is_err());
  drop(tx);
  assert!(!client.is_usable());
}

#[tokio::test(start_paused = true)]
async fn partially_written_secret_cancelled_with_slow_peer_invalidates() {
  use assuan_client::Event;
  use assuan_protocol::SecretRef;
  let (io, mut peer) = tokio::io::duplex(8);
  let (notification, receive) = tokio::sync::oneshot::channel();
  let remote = tokio::spawn(async move {
    peer.write_all(b"OK\n").await.unwrap();
    peer.read_exact(&mut [0; 4]).await.unwrap();
    peer.write_all(b"INQUIRE VALUE\n").await.unwrap();
    let mut prefix = [0; 2];
    peer.read_exact(&mut prefix).await.unwrap();
    assert_eq!(&prefix, b"D ");
    notification.send(()).unwrap();
    // Return ownership without draining; this intentionally applies backpressure.
    return peer;
  });
  let mut client = Client::from_stream(Stream::new(io), ClientOptions::default()).await.unwrap();
  let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  let Some(Event::Inquire(mut inquiry)) = tx.next().await.unwrap() else {
    panic!()
  };
  {
    let mut send = std::pin::pin!(inquiry.send_secret(SecretRef::new(&[0xFF; 100])));
    tokio::select! {
      result = &mut send => panic!("unexpected: {result:?}"),
      () = async { receive.await.unwrap(); } => {}
    }
    // Dropping the still-pending send leaves io_uncertain in SessionCore.
  }
  assert!(inquiry.send_data(b"retry").await.is_err());
  drop(inquiry);
  drop(tx);
  assert!(!client.is_usable());
  let mut peer = remote.await.unwrap();
  let mut remainder = Vec::new();
  peer.read_to_end(&mut remainder).await.unwrap();
  assert!(remainder.iter().all(|byte| return *byte == 0xFF));
  assert!(remainder.len() <= 8);
}

#[tokio::test(start_paused = true)]
async fn inquiry_deadline_is_the_minimum_of_local_and_enclosing_deadlines() {
  use assuan_client::{ClientError, Event};
  use assuan_transport::TransportError;
  use std::time::Duration;
  for (command, inquiry) in [(3, 10), (10, 3)] {
    let (io, mut peer) = tokio::io::duplex(128);
    peer.write_all(b"OK\n").await.unwrap();
    let options = ClientOptions {
      command_timeout: Duration::from_secs(command),
      inquiry_timeout: Duration::from_secs(inquiry),
      ..Default::default()
    };
    let mut client = Client::from_stream(Stream::new(io), options).await.unwrap();
    let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
    peer.read_exact(&mut [0; 4]).await.unwrap();
    peer.write_all(b"INQUIRE VALUE\n").await.unwrap();
    let Some(Event::Inquire(inquiry)) = tx.next().await.unwrap() else {
      panic!()
    };
    tokio::time::advance(Duration::from_secs(3)).await;
    assert!(matches!(inquiry.finish().await, Err(ClientError::Transport(TransportError::Timeout))));
    drop(tx);
    assert!(!client.is_usable());
    assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
  }
}

#[tokio::test(start_paused = true)]
async fn slowly_arriving_greeting_bytes_do_not_restart_its_deadline() {
  use assuan_client::ClientError;
  use assuan_transport::TransportError;
  use std::time::Duration;
  let (io, mut peer) = tokio::io::duplex(8);
  let remote = tokio::spawn(async move {
    for byte in b"OK\n" {
      tokio::time::sleep(Duration::from_secs(2)).await;
      if peer.write_all(&[*byte]).await.is_err() {
        break;
      }
    }
  });
  let options = ClientOptions {
    greeting_timeout: Duration::from_secs(5),
    ..Default::default()
  };
  assert!(matches!(
    Client::from_stream(Stream::new(io), options).await,
    Err(ClientError::Transport(TransportError::Timeout))
  ));
  remote.await.unwrap();
}
