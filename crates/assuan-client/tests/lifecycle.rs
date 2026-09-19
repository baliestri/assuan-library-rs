//! Session reuse, local validation, and abandonment against literal wire peers.
use assuan_client::{Client, ClientError, ClientOptions, Event, PayloadRef};
use assuan_protocol::{Command, ProtocolError};
use assuan_transport::Stream;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

async fn ready() -> (Client, DuplexStream) {
  let (io, mut peer) = tokio::io::duplex(2048);
  peer.write_all(b"# greeting\n\nS READY yes\nOK ready\n").await.unwrap();
  return (Client::from_stream(Stream::new(io), ClientOptions::default()).await.unwrap(), peer);
}

async fn assert_command(peer: &mut DuplexStream) {
  let mut bytes = [0; 4];
  peer.read_exact(&mut bytes).await.unwrap();
  assert_eq!(&bytes, b"NOP\n");
}

#[tokio::test]
async fn full_remote_error_code_preserves_reuse() {
  let (mut client, mut peer) = ready().await;
  let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  assert_command(&mut peer).await;
  peer.write_all(b"ERR 4294967295 private-message\n").await.unwrap();
  assert!(matches!(
    tx.next().await.unwrap(),
    Some(Event::Finished {
      code: Some(u32::MAX),
      ..
    })
  ));
  assert!(tx.next().await.unwrap().is_none());
  let error = tx.finish().unwrap_err();
  assert!(matches!(
    error,
    ClientError::Remote {
      code: u32::MAX
    }
  ));
  assert!(!format!("{error:?} {error}").contains("private-message"));
  assert!(client.is_usable());
  let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  assert_command(&mut peer).await;
  peer.write_all(b"OK\n").await.unwrap();
  tx.next().await.unwrap();
  tx.finish().unwrap();
}

#[tokio::test]
async fn greeting_error_closes_stream_and_preserves_code() {
  let (io, mut peer) = tokio::io::duplex(128);
  peer.write_all(b"ERR 4294967295 private-greeting\n").await.unwrap();
  let error = Client::from_stream(Stream::new(io), ClientOptions::default()).await.unwrap_err();
  assert!(matches!(
    error,
    ClientError::GreetingRejected {
      code: u32::MAX
    }
  ));
  assert!(!format!("{error:?}").contains("private-greeting"));
  assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
}

#[tokio::test]
async fn local_length_validation_does_not_write_or_poison_session() {
  let (mut client, mut peer) = ready().await;
  assert!(matches!(
    client.command(Command::new("NOP", &[b'x'; 1000]).unwrap()).await,
    Err(ClientError::Protocol(ProtocolError::LineTooLong))
  ));
  assert!(client.is_usable());
  let tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  assert_command(&mut peer).await;
  drop(tx);
  assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
}

#[tokio::test]
async fn premature_finish_drop_and_forget_prevent_another_write() {
  for mode in 0..3 {
    let (mut client, mut peer) = ready().await;
    let tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
    assert_command(&mut peer).await;
    match mode {
      0 => assert!(matches!(tx.finish(), Err(ClientError::Incomplete))),
      1 => drop(tx),
      _ => std::mem::forget(tx),
    }
    assert!(!client.is_usable());
    assert!(client.command(Command::new("NOP", b"").unwrap()).await.is_err());
    assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
  }
}

#[tokio::test]
async fn final_response_makes_drop_and_forget_safe() {
  for forget in [false, true] {
    let (mut client, mut peer) = ready().await;
    let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
    assert_command(&mut peer).await;
    peer.write_all(b"OK\n").await.unwrap();
    tx.next().await.unwrap();
    if forget {
      std::mem::forget(tx);
    } else {
      drop(tx);
    }
    assert!(client.is_usable());
    let tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
    assert_command(&mut peer).await;
    drop(tx);
  }
}

#[tokio::test]
async fn eof_without_final_and_malformed_responses_invalidate() {
  for bytes in [b"D hello\n".as_slice(), b"OKAY\n", b"D bad%GG\n", b"ERR x\n"] {
    let (mut client, mut peer) = ready().await;
    let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
    assert_command(&mut peer).await;
    peer.write_all(bytes).await.unwrap();
    drop(peer);
    if bytes == b"D hello\n" {
      assert!(matches!(tx.next().await.unwrap(), Some(Event::Data(_))));
    }
    assert!(tx.next().await.is_err());
    drop(tx);
    assert!(!client.is_usable());
  }
}

#[tokio::test]
async fn buffered_duplicate_final_cannot_complete_a_new_command() {
  let (mut client, mut peer) = ready().await;
  let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  assert_command(&mut peer).await;
  peer.write_all(b"OK\nOK duplicate\n").await.unwrap();
  tx.next().await.unwrap();
  tx.finish().unwrap();
  assert!(client.command(Command::new("NOP", b"").unwrap()).await.is_err());
  assert!(!client.is_usable());
  assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
}

#[tokio::test]
async fn buffered_comments_and_empty_lines_do_not_block_the_next_command() {
  let (mut client, mut peer) = ready().await;
  let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  assert_command(&mut peer).await;
  peer.write_all(b"OK\n# note\n\n").await.unwrap();
  tx.next().await.unwrap();
  tx.finish().unwrap();
  let tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  assert_command(&mut peer).await;
  drop(tx);
}

#[tokio::test]
async fn events_borrow_decoded_data_and_preserve_other_wire_fields() {
  let (mut client, mut peer) = ready().await;
  let mut tx = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  assert_command(&mut peer).await;
  peer
    .write_all(b"\nD a%00%0D%0A%FF\nS TEST a%20b\n# private-comment\nEND\nOK private-final\n")
    .await
    .unwrap();
  let Some(Event::Data(PayloadRef::Public(bytes))) = tx.next().await.unwrap() else {
    panic!()
  };
  assert_eq!(bytes, b"a\0\r\n\xff");
  {
    let status = tx.next().await.unwrap().unwrap();
    assert!(!format!("{status:?}").contains("TEST"));
    let Event::Status {
      keyword,
      args: PayloadRef::Public(args),
    } = status
    else {
      panic!()
    };
    assert_eq!(keyword, "TEST");
    assert_eq!(args, b"a%20b");
  }
  {
    let comment = tx.next().await.unwrap().unwrap();
    assert!(!format!("{comment:?}").contains("private"));
    assert!(matches!(comment, Event::Comment(PayloadRef::Public(b" private-comment"))));
  }
  assert!(matches!(tx.next().await.unwrap(), Some(Event::End)));
  assert!(matches!(
    tx.next().await.unwrap(),
    Some(Event::Finished {
      code: None,
      ..
    })
  ));
  tx.finish().unwrap();
}
