//! TCP failures, custom streams, short writes, and cancellation behavior.

use std::{error::Error, io, time::Duration};

use assuan_transport::{
  Acceptor, ConnectOptions, Endpoint, ListenOptions, Listener, LocalAccess, Stream, TransportError,
  connect,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn listener() -> Listener {
  return Listener::bind(&Endpoint::Tcp("127.0.0.1:0".parse().unwrap()), &ListenOptions::default())
    .await
    .unwrap();
}

#[tokio::test]
async fn assigned_endpoint_and_peer_metadata_are_explicit() {
  let mut listener = listener().await;
  let Endpoint::Tcp(address) = listener.endpoint() else {
    panic!("expected TCP")
  };
  assert!(address.ip().is_loopback());
  assert_ne!(address.port(), 0);
  let _client = connect(listener.endpoint(), &ConnectOptions::default()).await.unwrap();
  let accepted = listener.accept().await.unwrap();
  assert!(accepted.peer.is_none());
  assert_eq!(ListenOptions::default().local_access(), LocalAccess::CurrentUser);
  assert_eq!(ConnectOptions::default().timeout, Duration::from_secs(10));
}

#[tokio::test]
async fn tcp_shutdown_preserves_the_other_direction() {
  let mut listener = listener().await;
  let mut client = connect(listener.endpoint(), &ConnectOptions::default()).await.unwrap();
  let mut server = listener.accept().await.unwrap().stream;
  client.write_all(b"request").await.unwrap();
  client.shutdown().await.unwrap();
  let mut request = Vec::new();
  tokio::time::timeout(Duration::from_secs(5), server.read_to_end(&mut request))
    .await
    .unwrap()
    .unwrap();
  assert_eq!(request, b"request");
  server.write_all(b"response").await.unwrap();
  server.shutdown().await.unwrap();
  let mut response = Vec::new();
  tokio::time::timeout(Duration::from_secs(5), client.read_to_end(&mut response))
    .await
    .unwrap()
    .unwrap();
  assert_eq!(response, b"response");
}

#[tokio::test]
async fn refused_connection_retains_a_typed_io_cause() {
  let reserved = tokio::net::TcpSocket::new_v4().unwrap();
  reserved.bind("127.0.0.1:0".parse().unwrap()).unwrap();
  let endpoint = Endpoint::Tcp(reserved.local_addr().unwrap());
  let error = connect(
    &endpoint,
    &ConnectOptions {
      timeout: Duration::from_secs(5),
    },
  )
  .await
  .unwrap_err();
  let TransportError::Io(cause) = error else {
    panic!("expected I/O failure")
  };
  assert_eq!(cause.kind(), io::ErrorKind::ConnectionRefused);
}

#[tokio::test]
async fn zero_timeout_and_invalid_destination_are_distinct() {
  let listener = listener().await;
  let error = connect(
    listener.endpoint(),
    &ConnectOptions {
      timeout: Duration::ZERO,
    },
  )
  .await
  .unwrap_err();
  assert!(matches!(error, TransportError::Timeout));
  for address in ["127.0.0.1:0", "0.0.0.0:1234", "[::]:1234"] {
    let error = connect(&Endpoint::Tcp(address.parse().unwrap()), &ConnectOptions::default())
      .await
      .unwrap_err();
    assert!(matches!(error, TransportError::InvalidEndpoint));
  }
}

#[tokio::test]
async fn cancelled_accept_leaves_listener_usable() {
  let mut listener = listener().await;
  assert!(tokio::time::timeout(Duration::from_millis(10), listener.accept()).await.is_err());
  let mut client = connect(listener.endpoint(), &ConnectOptions::default()).await.unwrap();
  client.write_all(b"x").await.unwrap();
  let mut accepted = listener.accept().await.unwrap();
  let mut byte = [0];
  accepted.stream.read_exact(&mut byte).await.unwrap();
  assert_eq!(&byte, b"x");
}

#[tokio::test]
async fn custom_stream_propagates_short_writes_and_eof() {
  let (left, right) = tokio::io::duplex(2);
  let mut writer = Stream::new(left);
  let mut reader = Stream::new(right);
  assert_eq!(writer.write(b"abcd").await.unwrap(), 2);
  let mut first = [0; 2];
  reader.read_exact(&mut first).await.unwrap();
  assert_eq!(&first, b"ab");
  let payload: Vec<u8> = (0_u8..=255).cycle().take(4096).collect();
  let expected = payload.clone();
  let operation = async {
    let writing = async {
      writer.write_all(&payload).await.unwrap();
      writer.flush().await.unwrap();
      writer.shutdown().await.unwrap();
    };
    let reading = async {
      let mut bytes = Vec::new();
      reader.read_to_end(&mut bytes).await.unwrap();
      return bytes;
    };
    let ((), bytes) = tokio::join!(writing, reading);
    return bytes;
  };
  let bytes = tokio::time::timeout(Duration::from_secs(5), operation).await.unwrap();
  assert_eq!(bytes, expected);
}

#[tokio::test]
async fn dropped_peer_produces_eof_and_write_error() {
  let (left, right) = tokio::io::duplex(4);
  let mut stream = Stream::new(left);
  drop(right);
  assert_eq!(stream.read(&mut [0; 4]).await.unwrap(), 0);
  assert_eq!(stream.write(b"x").await.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
}

#[test]
fn transport_error_diagnostics_redact_external_text_but_preserve_the_source() {
  let error = TransportError::from(io::Error::other("private path and payload"));
  assert!(!format!("{error}").contains("private"));
  assert!(!format!("{error:?}").contains("private"));
  assert_eq!(error.source().unwrap().to_string(), "private path and payload");
}

#[test]
fn wrapper_is_send_and_unpin_without_requiring_inner_debug() {
  fn check<T: Send + Unpin>(_: &T) {}
  let (left, _) = tokio::io::duplex(4);
  let stream = Stream::new(left);
  check(&stream);
  assert_eq!(format!("{stream:?}"), "Stream { .. }");
}
