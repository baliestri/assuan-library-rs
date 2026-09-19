//! Interactive and automatic greetings with exact END/CAN exchanges.
use assuan_client::{
  Client, ClientError, ClientFuture, ClientOptions, Event, GreetingHandler, Inquiry,
};
use assuan_transport::{Endpoint, Stream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn manual_handshake_answers_inquiry_before_becoming_usable() {
  let (io, mut peer) = tokio::io::duplex(128);
  let mut handshake = Client::handshake(Stream::new(io), ClientOptions::default());
  peer.write_all(b"S START yes\nINQUIRE AUTH challenge\n").await.unwrap();
  assert!(matches!(handshake.next().await.unwrap(), Some(Event::Status { .. })));
  let Some(Event::Inquire(mut inquiry)) = handshake.next().await.unwrap() else {
    panic!()
  };
  assert_eq!(inquiry.keyword(), "AUTH");
  inquiry.send_data(b"answer").await.unwrap();
  inquiry.finish().await.unwrap();
  let mut sent = [0; 13];
  peer.read_exact(&mut sent).await.unwrap();
  assert_eq!(&sent, b"D answer\nEND\n");
  peer.write_all(b"OK welcome\n").await.unwrap();
  assert!(matches!(
    handshake.next().await.unwrap(),
    Some(Event::Finished {
      code: None,
      ..
    })
  ));
  assert!(handshake.next().await.unwrap().is_none());
  assert!(handshake.finish().unwrap().is_usable());
}

#[tokio::test]
async fn default_greeting_cancels_then_waits_for_final_ok_or_err() {
  for success in [false, true] {
    let (io, mut peer) = tokio::io::duplex(128);
    let remote = tokio::spawn(async move {
      peer.write_all(b"INQUIRE AUTH private\n").await.unwrap();
      let mut sent = [0; 4];
      peer.read_exact(&mut sent).await.unwrap();
      assert_eq!(&sent, b"CAN\n");
      peer
        .write_all(if success {
          b"OK\n"
        } else {
          b"ERR 4294967295 cancelled\n"
        })
        .await
        .unwrap();
    });
    let result = Client::from_stream(Stream::new(io), ClientOptions::default()).await;
    if success {
      assert!(result.unwrap().is_usable());
    } else {
      assert!(matches!(
        result,
        Err(ClientError::GreetingRejected {
          code: u32::MAX
        })
      ));
    }
    remote.await.unwrap();
  }
}

#[tokio::test]
async fn abandoned_handshake_and_invalid_options_close_the_connection() {
  let (io, mut peer) = tokio::io::duplex(128);
  let handshake = Client::handshake(Stream::new(io), ClientOptions::default());
  assert!(matches!(handshake.finish(), Err(ClientError::Incomplete)));
  assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
  let (io, mut peer) = tokio::io::duplex(128);
  let options = ClientOptions {
    max_inquiry_bytes: 0,
    ..Default::default()
  };
  let mut handshake = Client::handshake(Stream::new(io), options);
  assert!(matches!(handshake.next().await, Err(ClientError::InvalidOptions)));
  assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
}

struct Handler {
  fail: bool,
}

impl GreetingHandler for Handler {
  fn respond<'a>(&'a mut self, mut inquiry: Inquiry<'a>) -> ClientFuture<'a, ()> {
    return Box::pin(async move {
      if self.fail {
        return Err(ClientError::Incomplete);
      }
      assert_eq!(inquiry.keyword(), "AUTH");
      inquiry.send_data(b"answer").await?;
      return inquiry.finish().await;
    });
  }
}

#[tokio::test]
async fn connector_callback_success_and_failure_have_explicit_outcomes() {
  for fail in [false, true] {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = Endpoint::Tcp(listener.local_addr().unwrap());
    let remote = tokio::spawn(async move {
      let (mut peer, _) = listener.accept().await.unwrap();
      peer.write_all(b"INQUIRE AUTH challenge\n").await.unwrap();
      if fail {
        assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
      } else {
        let mut sent = [0; 13];
        peer.read_exact(&mut sent).await.unwrap();
        assert_eq!(&sent, b"D answer\nEND\n");
        peer.write_all(b"OK\n").await.unwrap();
      }
    });
    let result = Client::connect_with(
      &endpoint,
      ClientOptions::default(),
      &mut Handler {
        fail,
      },
    )
    .await;
    if fail {
      assert!(matches!(result, Err(ClientError::Incomplete)));
    } else {
      assert!(result.unwrap().is_usable());
    }
    remote.await.unwrap();
  }
}
