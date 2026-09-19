//! Windows byte pipes support successive accepted connections.
#![cfg(windows)]

#[tokio::test]
async fn named_pipe_supports_successive_connections() {
  use assuan_transport::{Acceptor, ConnectOptions, Endpoint, ListenOptions, Listener};
  use tokio::io::{AsyncReadExt, AsyncWriteExt};
  let unique = tempfile::tempdir().unwrap();
  let name = unique.path().file_name().unwrap().to_string_lossy();
  let endpoint =
    Endpoint::NamedPipe(format!(r"\\.\pipe\assuan-test-{}-{name}", std::process::id()).into());
  let mut listener = Listener::bind(&endpoint, &ListenOptions::default()).await.unwrap();
  for byte in [1_u8, 2] {
    let mut client =
      assuan_transport::connect(&endpoint, &ConnectOptions::default()).await.unwrap();
    let mut server = listener.accept().await.unwrap().stream;
    client.write_all(&[byte]).await.unwrap();
    let mut out = [0];
    server.read_exact(&mut out).await.unwrap();
    assert_eq!(out, [byte]);
  }
}
