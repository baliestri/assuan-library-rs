//! TCP transports preserve arbitrary binary bytes.

#[tokio::test]
async fn tcp_preserves_binary_bytes() {
  use assuan_transport::{Acceptor, ConnectOptions, Endpoint, ListenOptions, Listener};
  use tokio::io::{AsyncReadExt, AsyncWriteExt};
  let mut listener =
    Listener::bind(&Endpoint::Tcp("127.0.0.1:0".parse().unwrap()), &ListenOptions::default())
      .await
      .unwrap();
  let mut client =
    assuan_transport::connect(listener.endpoint(), &ConnectOptions::default()).await.unwrap();
  let mut peer = listener.accept().await.unwrap().stream;
  client.write_all(b"\0\xff\n").await.unwrap();
  let mut out = [0; 3];
  peer.read_exact(&mut out).await.unwrap();
  assert_eq!(&out, b"\0\xff\n");
}
