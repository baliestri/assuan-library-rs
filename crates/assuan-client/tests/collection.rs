//! Bounded response retention for public and protected data.
use assuan_client::{Client, ClientError, ClientOptions, CollectLimits, PayloadRef};
use assuan_protocol::{Command, SecretRef, Sensitivity};
use assuan_transport::Stream;
use tokio::io::{AsyncWriteExt, DuplexStream};

async fn ready() -> (Client, DuplexStream) {
  let (io, mut peer) = tokio::io::duplex(4096);
  peer.write_all(b"OK\n").await.unwrap();
  return (Client::from_stream(Stream::new(io), ClientOptions::default()).await.unwrap(), peer);
}

#[test]
fn collection_limits_are_finite() {
  let limits = CollectLimits::default();
  assert_eq!(limits.max_bytes(), 16 * 1024 * 1024);
  assert_eq!(limits.max_events(), 4096);
  assert!(CollectLimits::new(0, 1).is_err());
  assert!(CollectLimits::new(1, 0).is_err());
}

#[tokio::test]
async fn public_collection_concatenates_data_and_retains_metadata() {
  let (mut client, mut peer) = ready().await;
  let remote = tokio::spawn(async move {
    peer.write_all(b"D one\nD two\nS INFO details\n# comment\nEND\nOK\n").await.unwrap();
    return peer;
  });
  let response = client
    .collect(Command::new("NOP", b"").unwrap(), CollectLimits::new(64, 10).unwrap())
    .await
    .unwrap();
  assert_eq!(response.data(), b"onetwo");
  assert_eq!(response.statuses()[0].0, "INFO");
  assert_eq!(response.statuses()[0].1, b"details");
  assert_eq!(response.comments()[0], b" comment");
  assert!(client.is_usable());
  let _peer = remote.await.unwrap();
}

#[tokio::test]
async fn limits_apply_to_all_retained_fields_and_event_count() {
  let (mut client, mut peer) = ready().await;
  peer.write_all(b"D ab\nOK\n").await.unwrap();
  let error = client
    .collect(Command::new("NOP", b"").unwrap(), CollectLimits::new(1, 10).unwrap())
    .await
    .unwrap_err();
  assert!(matches!(error, ClientError::Limit(_)));
  assert!(!client.is_usable());

  let (mut client, mut peer) = ready().await;
  peer.write_all(b"# a\n# b\nOK\n").await.unwrap();
  let error = client
    .collect(Command::new("NOP", b"").unwrap(), CollectLimits::new(64, 1).unwrap())
    .await
    .unwrap_err();
  assert!(matches!(error, ClientError::Limit(_)));
  assert!(!client.is_usable());
}

#[tokio::test]
async fn secret_collection_exposes_only_secret_ref_and_wipes_on_drop() {
  let (mut client, mut peer) = ready().await;
  peer.write_all(b"D top%20secret\nOK\n").await.unwrap();
  let response = client
    .collect_secret(Command::new("NOP", b"").unwrap(), CollectLimits::new(64, 4).unwrap())
    .await
    .unwrap();
  assert_eq!(response.data().expose(), b"top secret");
  assert!(!format!("{response:?}").contains("secret"));
}

#[tokio::test]
async fn public_and_secret_command_classification_are_selected_before_send() {
  let (mut client, mut peer) = ready().await;
  peer.write_all(b"D value\nOK\n").await.unwrap();
  let mut tx =
    client.command_with(Command::new("NOP", b"").unwrap(), Sensitivity::Secret).await.unwrap();
  let Some(assuan_client::Event::Data(PayloadRef::Secret(secret))) = tx.next().await.unwrap()
  else {
    panic!()
  };
  assert_eq!(secret.expose(), b"value");
  tx.next().await.unwrap();
  tx.finish().unwrap();
  let _ = SecretRef::new(b"caller-owned");
}

#[tokio::test]
async fn metadata_before_data_counts_toward_both_collection_limits() {
  for secret in [false, true] {
    for wire in [b"S INFO x\nD abc\nOK\n".as_slice(), b"# abcde\nD abc\nOK\n"] {
      let (mut client, mut peer) = ready().await;
      peer.write_all(wire).await.unwrap();
      let command = Command::new("NOP", b"").unwrap();
      let limits = CollectLimits::new(8, 10).unwrap();
      let error = if secret {
        client.collect_secret(command, limits).await.unwrap_err()
      } else {
        client.collect(command, limits).await.unwrap_err()
      };
      assert!(matches!(error, ClientError::Limit(_)));
      assert!(!client.is_usable());
    }
  }
}

#[tokio::test]
async fn collected_metadata_debug_is_redacted() {
  for secret in [false, true] {
    let (mut client, mut peer) = ready().await;
    peer.write_all(b"S PRIVATE sensitive\n# sensitive\nD sensitive\nOK\n").await.unwrap();
    let command = Command::new("NOP", b"").unwrap();
    let limits = CollectLimits::new(64, 8).unwrap();
    let debug = if secret {
      format!("{:?}", client.collect_secret(command, limits).await.unwrap())
    } else {
      format!("{:?}", client.collect(command, limits).await.unwrap())
    };
    assert!(!debug.contains("PRIVATE"));
    assert!(!debug.contains("sensitive"));
    assert!(!debug.contains("115, 101, 110"));
    assert!(debug.contains("status_count: 1"));
  }
}

#[tokio::test]
async fn interleaved_metadata_and_data_accept_exact_retention_budget() {
  for secret in [false, true] {
    let (mut client, mut peer) = ready().await;
    peer.write_all(b"D a\nS I x\n#z\nD bc\nOK\n").await.unwrap();
    let command = Command::new("NOP", b"").unwrap();
    let limits = CollectLimits::new(7, 5).unwrap();
    if secret {
      let response = client.collect_secret(command, limits).await.unwrap();
      assert_eq!(response.data().expose(), b"abc");
    } else {
      let response = client.collect(command, limits).await.unwrap();
      assert_eq!(response.data(), b"abc");
    }
    assert!(client.is_usable());
  }
}
