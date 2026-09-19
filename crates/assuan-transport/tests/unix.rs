//! Unix endpoints preserve existing paths and enforce local-user access.
#![cfg(unix)]

#[tokio::test]
async fn unix_bind_never_replaces_a_file() {
  use assuan_transport::{Endpoint, ListenOptions, Listener};
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("agent.sock");
  std::fs::write(&path, b"keep").unwrap();
  assert!(Listener::bind(&Endpoint::Unix(path.clone()), &ListenOptions::default()).await.is_err());
  assert_eq!(std::fs::read(path).unwrap(), b"keep");
}
