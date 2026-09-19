//! Independent literal peers exercise client protocol behavior.
use assuan_client::{Client, ClientOptions, Event};
use assuan_protocol::Command;
use assuan_transport::Stream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn command_finishes_on_ok_not_on_idle() {
  let (io, mut peer) = tokio::io::duplex(128);
  let remote = tokio::spawn(async move {
    peer.write_all(b"OK ready\n").await.unwrap();
    let mut cmd = [0; 4];
    peer.read_exact(&mut cmd).await.unwrap();
    assert_eq!(&cmd, b"NOP\n");
    peer.write_all(b"D hello\nOK\n").await.unwrap();
  });
  let mut client = Client::from_stream(Stream::new(io), ClientOptions::default()).await.unwrap();
  let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  assert!(matches!(tx.next().await.unwrap(), Some(Event::Data(_))));
  assert!(matches!(
    tx.next().await.unwrap(),
    Some(Event::Finished {
      code: None,
      ..
    })
  ));
  tx.finish().unwrap();
  assert!(client.is_usable());
  remote.await.unwrap();
}
