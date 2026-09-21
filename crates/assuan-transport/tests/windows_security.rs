//! Named-pipe lifecycle, deadlines, and invalid endpoint handling.
#![cfg(windows)]

use std::{io, time::Duration};

use assuan_transport::{
  Acceptor, ConnectOptions, Endpoint, ListenOptions, Listener, TransportError, connect,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn endpoint() -> (tempfile::TempDir, Endpoint) {
  let unique = tempfile::tempdir().unwrap();
  let name = unique.path().file_name().unwrap().to_string_lossy();
  let endpoint =
    Endpoint::NamedPipe(format!(r"\\.\pipe\assuan-security-{}-{name}", std::process::id()).into());
  return (unique, endpoint);
}

#[tokio::test]
async fn existing_pipe_name_is_rejected_without_disturbing_the_listener() {
  let (_unique, endpoint) = endpoint();
  let mut first = Listener::bind(&endpoint, &ListenOptions::default()).await.unwrap();
  assert!(Listener::bind(&endpoint, &ListenOptions::default()).await.is_err());
  let mut client = connect(&endpoint, &ConnectOptions::default()).await.unwrap();
  let mut accepted = first.accept().await.unwrap();
  assert!(accepted.peer.is_none());
  client.write_all(b"\0\xff\n").await.unwrap();
  let mut bytes = [0; 3];
  accepted.stream.read_exact(&mut bytes).await.unwrap();
  assert_eq!(&bytes, b"\0\xff\n");
}

#[tokio::test]
async fn busy_pipe_times_out_without_leaving_a_background_connection() {
  let (_unique, endpoint) = endpoint();
  let mut listener = Listener::bind(&endpoint, &ListenOptions::default()).await.unwrap();
  let _first = connect(&endpoint, &ConnectOptions::default()).await.unwrap();
  let error = connect(
    &endpoint,
    &ConnectOptions {
      timeout: Duration::from_millis(30),
    },
  )
  .await
  .unwrap_err();
  assert!(matches!(error, TransportError::Timeout));
  let _accepted_first = listener.accept().await.unwrap();
  let mut second = connect(&endpoint, &ConnectOptions::default()).await.unwrap();
  second.write_all(b"second").await.unwrap();
  let mut accepted_second = listener.accept().await.unwrap().stream;
  let mut bytes = [0; 6];
  tokio::time::timeout(Duration::from_secs(2), accepted_second.read_exact(&mut bytes))
    .await
    .unwrap()
    .unwrap();
  assert_eq!(&bytes, b"second");
}

#[tokio::test]
async fn cancelling_accept_preserves_future_connections() {
  let (_unique, endpoint) = endpoint();
  let mut listener = Listener::bind(&endpoint, &ListenOptions::default()).await.unwrap();
  assert!(tokio::time::timeout(Duration::from_millis(10), listener.accept()).await.is_err());
  let options = ConnectOptions::default();
  let connecting = connect(&endpoint, &options);
  let (accepted, client) = tokio::join!(listener.accept(), connecting);
  assert!(accepted.is_ok());
  assert!(client.is_ok());
}

#[tokio::test]
async fn flush_and_shutdown_do_not_wait_for_peer_reads() {
  let (_unique, endpoint) = endpoint();
  let mut listener = Listener::bind(&endpoint, &ListenOptions::default()).await.unwrap();
  let mut client = connect(&endpoint, &ConnectOptions::default()).await.unwrap();
  let _unread_peer = listener.accept().await.unwrap();
  client.write_all(b"unread data").await.unwrap();
  tokio::time::timeout(Duration::from_millis(100), client.flush()).await.unwrap().unwrap();
  tokio::time::timeout(Duration::from_millis(100), client.shutdown()).await.unwrap().unwrap();
  assert_eq!(client.write(b"later").await.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
  assert_eq!(client.read(&mut [0]).await.unwrap(), 0);
}

#[tokio::test]
async fn dropping_a_connected_pipe_notifies_the_other_end() {
  let (_unique, endpoint) = endpoint();
  let mut listener = Listener::bind(&endpoint, &ListenOptions::default()).await.unwrap();
  let client = connect(&endpoint, &ConnectOptions::default()).await.unwrap();
  let mut server = listener.accept().await.unwrap().stream;
  drop(client);
  let read = tokio::time::timeout(Duration::from_secs(2), server.read(&mut [0])).await.unwrap();
  assert!(matches!(read, Ok(0)) || read.is_err());
}

#[tokio::test]
async fn remote_empty_and_malformed_pipe_paths_are_rejected() {
  for path in [
    r"\\remote\pipe\test",
    r"\\.\pipe\",
    r"C:\test",
    "\\\\.\\pipe\\bad\0name",
    r"\\.\pipe\nested\name",
  ] {
    let endpoint = Endpoint::NamedPipe(path.into());
    assert!(matches!(
      Listener::bind(&endpoint, &ListenOptions::default()).await,
      Err(TransportError::InvalidEndpoint)
    ));
    assert!(matches!(
      connect(&endpoint, &ConnectOptions::default()).await,
      Err(TransportError::InvalidEndpoint)
    ));
  }
}
