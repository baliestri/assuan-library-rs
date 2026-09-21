//! Interoperability with an independently implemented agent.
mod support;

use assuan_library::{Client, ClientError, ClientOptions, CollectLimits, Command, ConnectOptions};

#[tokio::test]
async fn agent_getinfo_returns_a_version() {
  let Some(mut fixture) = support::gnupg::start_or_skip().await.unwrap() else {
    return;
  };
  let resolved = fixture.locator().resolve().await.unwrap();
  let stream = resolved.connect(&ConnectOptions::default()).await.unwrap();
  let mut client = Client::from_stream(stream, ClientOptions::default()).await.unwrap();
  let response = client
    .collect(Command::new("GETINFO", b"version").unwrap(), CollectLimits::default())
    .await
    .unwrap();
  assert!(!response.data().is_empty());
  client.collect(Command::new("NOP", b"").unwrap(), CollectLimits::default()).await.unwrap();
  let error = client
    .collect(Command::new("ASSUAN_TEST_UNKNOWN", b"").unwrap(), CollectLimits::default())
    .await
    .unwrap_err();
  assert!(matches!(error, ClientError::Remote { .. }));
  assert!(client.is_usable());
  client.collect(Command::new("NOP", b"").unwrap(), CollectLimits::default()).await.unwrap();
  client.collect(Command::new("BYE", b"").unwrap(), CollectLimits::default()).await.unwrap();
  drop(client);
  let home = fixture.home().to_owned();
  fixture.shutdown().await.unwrap();
  assert!(!home.exists());
}

#[tokio::test]
async fn dropping_fixture_stops_its_agent_and_removes_its_home() {
  let Some(fixture) = support::gnupg::start_or_skip().await.unwrap() else {
    return;
  };
  let home = fixture.home().to_owned();
  let resolved = fixture.locator().resolve().await.unwrap();
  drop(fixture);
  assert!(!home.exists());
  assert!(resolved.connect(&ConnectOptions::default()).await.is_err());
}

#[tokio::test]
async fn literal_tcp_peer_exercises_greeting_inquiry_and_secret_binary_data() {
  use assuan_library::{Event, PayloadRef, SecretRef, Sensitivity, Stream};
  use tokio::io::{AsyncReadExt, AsyncWriteExt};
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let peer = tokio::spawn(async move {
    let (mut io, _) = listener.accept().await.unwrap();
    io.write_all(b"INQUIRE AUTH challenge\n").await.unwrap();
    let mut answer = [0; 20];
    io.read_exact(&mut answer).await.unwrap();
    assert_eq!(&answer, b"D %00%0D%0A%25\xff\nEND\n");
    io.write_all(b"OK\n").await.unwrap();
    let mut command = [0; 5];
    io.read_exact(&mut command).await.unwrap();
    assert_eq!(&command, b"READ\n");
    io.write_all(b"D %00%0D%0A%25\xff\nOK\n").await.unwrap();
  });
  let stream = tokio::net::TcpStream::connect(address).await.unwrap();
  let mut handshake = Client::handshake(Stream::new(stream), ClientOptions::default());
  let Some(Event::Inquire(mut inquiry)) = handshake.next().await.unwrap() else {
    panic!("expected greeting inquiry");
  };
  inquiry.send_secret(SecretRef::new(b"\0\r\n%\xff")).await.unwrap();
  inquiry.finish().await.unwrap();
  assert!(matches!(
    handshake.next().await.unwrap(),
    Some(Event::Finished {
      code: None,
      ..
    })
  ));
  let mut client = handshake.finish().unwrap();
  let mut transaction =
    client.command_with(Command::new("READ", b"").unwrap(), Sensitivity::Secret).await.unwrap();
  {
    let event = transaction.next().await.unwrap().unwrap();
    assert_eq!(format!("{event:?}"), "Data { .. }");
    let Event::Data(PayloadRef::Secret(data)) = event else {
      panic!("expected classified secret");
    };
    assert_eq!(data.expose(), b"\0\r\n%\xff");
  }
  assert!(matches!(
    transaction.next().await.unwrap(),
    Some(Event::Finished {
      code: None,
      ..
    })
  ));
  transaction.finish().unwrap();
  peer.await.unwrap();
}
