//! Literal inquiry exchanges over independent byte streams.
use assuan_client::{Client, ClientOptions, Event};
use assuan_protocol::Command;
use assuan_transport::Stream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn inquiry_sends_data_and_end_before_final_response() {
  let (io, mut peer) = tokio::io::duplex(128);
  peer.write_all(b"OK\n").await.unwrap();
  let mut client = Client::from_stream(Stream::new(io), ClientOptions::default()).await.unwrap();
  let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  let mut command = [0; 4];
  peer.read_exact(&mut command).await.unwrap();
  assert_eq!(&command, b"NOP\n");
  peer.write_all(b"INQUIRE VALUE name\n").await.unwrap();
  let Some(Event::Inquire(mut inquiry)) = tx.next().await.unwrap() else {
    panic!()
  };
  assert_eq!(inquiry.keyword(), "VALUE");
  inquiry.send_data(b"a\n").await.unwrap();
  inquiry.finish().await.unwrap();
  let mut answer = [0; 11];
  peer.read_exact(&mut answer).await.unwrap();
  assert_eq!(&answer, b"D a%0A\nEND\n");
  peer.write_all(b"OK\n").await.unwrap();
  assert!(matches!(
    tx.next().await.unwrap(),
    Some(Event::Finished {
      code: None,
      ..
    })
  ));
  tx.finish().unwrap();
  assert!(client.is_usable());
}

#[tokio::test]
async fn cancelled_inquiry_requires_final_error_and_preserves_reuse() {
  let (io, mut peer) = tokio::io::duplex(128);
  peer.write_all(b"OK\n").await.unwrap();
  let mut client = Client::from_stream(Stream::new(io), ClientOptions::default()).await.unwrap();
  let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  peer.read_exact(&mut [0; 4]).await.unwrap();
  peer.write_all(b"INQUIRE VALUE\n").await.unwrap();
  let Some(Event::Inquire(inquiry)) = tx.next().await.unwrap() else {
    panic!()
  };
  inquiry.cancel().await.unwrap();
  let mut answer = [0; 4];
  peer.read_exact(&mut answer).await.unwrap();
  assert_eq!(&answer, b"CAN\n");
  peer.write_all(b"S CANCELLED yes\nERR 4294967295 cancelled\n").await.unwrap();
  assert!(matches!(tx.next().await.unwrap(), Some(Event::Status { .. })));
  assert!(matches!(
    tx.next().await.unwrap(),
    Some(Event::Finished {
      code: Some(u32::MAX),
      ..
    })
  ));
  assert!(matches!(
    tx.finish(),
    Err(assuan_client::ClientError::Remote {
      code: u32::MAX
    })
  ));
  assert!(client.is_usable());
  let tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  peer.read_exact(&mut answer).await.unwrap();
  assert_eq!(&answer, b"NOP\n");
  drop(tx);
}

#[tokio::test]
async fn inquiry_drop_and_forget_cannot_be_bypassed_by_a_queued_final() {
  for forget in [false, true] {
    let (io, mut peer) = tokio::io::duplex(128);
    peer.write_all(b"OK\n").await.unwrap();
    let mut client = Client::from_stream(Stream::new(io), ClientOptions::default()).await.unwrap();
    let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
    peer.read_exact(&mut [0; 4]).await.unwrap();
    peer.write_all(b"INQUIRE VALUE\nOK\n").await.unwrap();
    let Some(Event::Inquire(inquiry)) = tx.next().await.unwrap() else {
      panic!()
    };
    if forget {
      std::mem::forget(inquiry);
    } else {
      drop(inquiry);
    }
    assert!(tx.next().await.is_err());
    drop(tx);
    assert!(!client.is_usable());
    assert!(client.command(Command::new("NOP", b"").unwrap()).await.is_err());
    assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
  }
}

#[tokio::test]
async fn secret_metadata_and_response_remain_explicitly_classified() {
  use assuan_client::PayloadRef;
  use assuan_protocol::{SecretRef, Sensitivity};
  let (io, mut peer) = tokio::io::duplex(128);
  peer.write_all(b"OK\n").await.unwrap();
  let mut client = Client::from_stream(Stream::new(io), ClientOptions::default()).await.unwrap();
  let mut tx =
    client.command_with(Command::new("NOP", b"").unwrap(), Sensitivity::Secret).await.unwrap();
  peer.read_exact(&mut [0; 4]).await.unwrap();
  peer.write_all(b"INQUIRE TOKEN private-args\n").await.unwrap();
  let Some(Event::Inquire(mut inquiry)) = tx.next().await.unwrap() else {
    panic!()
  };
  let PayloadRef::Secret(args) = inquiry.args() else {
    panic!()
  };
  assert_eq!(args.expose(), b"private-args");
  assert!(!format!("{inquiry:?}").contains("private-args"));
  inquiry.send_secret(SecretRef::new(b"\0\r\n%\xff")).await.unwrap();
  inquiry.finish().await.unwrap();
  let mut response = [0; 20];
  peer.read_exact(&mut response).await.unwrap();
  assert_eq!(&response, b"D %00%0D%0A%25\xff\nEND\n");
  peer
    .write_all(b"D private%00data\nS TOKEN private-status\n# private-comment\nOK private-final\n")
    .await
    .unwrap();
  {
    let Some(Event::Data(PayloadRef::Secret(data))) = tx.next().await.unwrap() else {
      panic!()
    };
    assert_eq!(data.expose(), b"private\0data");
  }
  assert!(matches!(
    tx.next().await.unwrap(),
    Some(Event::Status {
      args: PayloadRef::Secret(_),
      ..
    })
  ));
  assert!(matches!(tx.next().await.unwrap(), Some(Event::Comment(PayloadRef::Secret(_)))));
  assert!(matches!(
    tx.next().await.unwrap(),
    Some(Event::Finished {
      text: PayloadRef::Secret(_),
      ..
    })
  ));
  tx.finish().unwrap();
  // A later ordinary command explicitly restores public classification.
  let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  peer.read_exact(&mut [0; 4]).await.unwrap();
  peer.write_all(b"OK public\n").await.unwrap();
  assert!(matches!(
    tx.next().await.unwrap(),
    Some(Event::Finished {
      text: PayloadRef::Public(b"public"),
      ..
    })
  ));
  tx.finish().unwrap();
}

#[tokio::test]
async fn limit_counts_raw_bytes_and_rejects_an_entire_excess_call() {
  let (io, mut peer) = tokio::io::duplex(128);
  peer.write_all(b"OK\n").await.unwrap();
  let options = ClientOptions {
    max_inquiry_bytes: 3,
    ..Default::default()
  };
  let mut client = Client::from_stream(Stream::new(io), options).await.unwrap();
  let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  peer.read_exact(&mut [0; 4]).await.unwrap();
  peer.write_all(b"INQUIRE VALUE\n").await.unwrap();
  let Some(Event::Inquire(mut inquiry)) = tx.next().await.unwrap() else {
    panic!()
  };
  inquiry.send_data(b"\0\r\n").await.unwrap();
  let mut sent = [0; 12];
  peer.read_exact(&mut sent).await.unwrap();
  assert_eq!(&sent, b"D %00%0D%0A\n");
  assert!(matches!(inquiry.send_data(b"x").await, Err(assuan_client::ClientError::Limit(_))));
  drop(inquiry);
  drop(tx);
  assert!(!client.is_usable());
  assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
}

#[tokio::test]
async fn multiple_inquiries_have_independent_budgets() {
  let (io, mut peer) = tokio::io::duplex(128);
  peer.write_all(b"OK\n").await.unwrap();
  let options = ClientOptions {
    max_inquiry_bytes: 1,
    ..Default::default()
  };
  let mut client = Client::from_stream(Stream::new(io), options).await.unwrap();
  let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  peer.read_exact(&mut [0; 4]).await.unwrap();
  for _ in 0..2 {
    peer.write_all(b"INQUIRE VALUE\n").await.unwrap();
    let Some(Event::Inquire(mut inquiry)) = tx.next().await.unwrap() else {
      panic!()
    };
    inquiry.send_data(b"").await.unwrap();
    inquiry.send_data(b"x").await.unwrap();
    inquiry.finish().await.unwrap();
    let mut sent = [0; 11];
    peer.read_exact(&mut sent).await.unwrap();
    assert_eq!(&sent, b"D \nD x\nEND\n");
  }
  peer.write_all(b"OK\n").await.unwrap();
  tx.next().await.unwrap();
  tx.finish().unwrap();
}

#[tokio::test]
async fn large_raw_data_is_split_without_breaking_escapes() {
  let (io, mut peer) = tokio::io::duplex(32);
  let remote = tokio::spawn(async move {
    peer.write_all(b"OK\n").await.unwrap();
    peer.read_exact(&mut [0; 4]).await.unwrap();
    peer.write_all(b"INQUIRE VALUE\n").await.unwrap();
    let mut bytes = vec![0; 3012];
    peer.read_exact(&mut bytes).await.unwrap();
    let expected = [
      b"D ".as_slice(),
      &b"%00".repeat(332),
      b"\nD ",
      &b"%00".repeat(332),
      b"\nD ",
      &b"%00".repeat(332),
      b"\nD ",
      &b"%00".repeat(4),
      b"\n",
    ]
    .concat();
    assert_eq!(bytes, expected);
    let mut end = [0; 4];
    peer.read_exact(&mut end).await.unwrap();
    assert_eq!(&end, b"END\n");
    peer.write_all(b"OK\n").await.unwrap();
  });
  let mut client = Client::from_stream(Stream::new(io), ClientOptions::default()).await.unwrap();
  let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  let Some(Event::Inquire(mut inquiry)) = tx.next().await.unwrap() else {
    panic!()
  };
  inquiry.send_data(&[0; 1000]).await.unwrap();
  inquiry.finish().await.unwrap();
  tx.next().await.unwrap();
  tx.finish().unwrap();
  remote.await.unwrap();
}
