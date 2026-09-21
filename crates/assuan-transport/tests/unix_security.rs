//! Filesystem identity, local credentials, and cleanup regressions.
#![cfg(unix)]

use std::{
  fs,
  os::unix::fs::{PermissionsExt, symlink},
};

use assuan_transport::{
  Acceptor, ConnectOptions, Endpoint, ListenOptions, Listener, PeerIdentity, TransportError,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn private_dir() -> tempfile::TempDir {
  let directory = tempfile::tempdir().unwrap();
  fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
  return directory;
}

#[tokio::test]
async fn unix_bytes_credentials_and_cleanup() {
  let directory = private_dir();
  let path = directory.path().join("socket");
  let endpoint = Endpoint::Unix(path.clone());
  let mut listener = Listener::bind(&endpoint, &ListenOptions::default()).await.unwrap();
  assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
  let mut client =
    assuan_transport::connect(listener.endpoint(), &ConnectOptions::default()).await.unwrap();
  let mut accepted = listener.accept().await.unwrap();
  let Some(PeerIdentity::Unix {
    uid,
    gid,
    ..
  }) = accepted.peer
  else {
    panic!("missing OS credentials")
  };
  assert_eq!(uid, rustix::process::geteuid().as_raw());
  assert_eq!(gid, rustix::process::getegid().as_raw());
  client.write_all(b"\0\xff\r\n").await.unwrap();
  let mut bytes = [0; 4];
  accepted.stream.read_exact(&mut bytes).await.unwrap();
  assert_eq!(&bytes, b"\0\xff\r\n");
  listener.cleanup().unwrap();
  assert!(!path.exists());
  assert!(listener.accept().await.is_err());
  listener.cleanup().unwrap();
  fs::write(&path, b"new owner").unwrap();
  listener.cleanup().unwrap();
  assert_eq!(fs::read(&path).unwrap(), b"new owner");
}

#[tokio::test]
async fn permissive_parent_is_rejected_even_for_root() {
  let directory = private_dir();
  fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
  let path = directory.path().join("socket");
  let result = Listener::bind(&Endpoint::Unix(path.clone()), &ListenOptions::default()).await;
  assert!(matches!(result, Err(TransportError::AccessPolicy)));
  assert!(!path.exists());
}

#[tokio::test]
async fn existing_socket_is_never_replaced() {
  let directory = private_dir();
  let path = directory.path().join("socket");
  let endpoint = Endpoint::Unix(path.clone());
  let mut first = Listener::bind(&endpoint, &ListenOptions::default()).await.unwrap();
  assert!(Listener::bind(&endpoint, &ListenOptions::default()).await.is_err());
  let _client = assuan_transport::connect(&endpoint, &ConnectOptions::default()).await.unwrap();
  assert!(first.accept().await.is_ok());
  first.cleanup().unwrap();
}

#[tokio::test]
async fn endpoint_and_direct_parent_symlinks_are_rejected() {
  let directory = private_dir();
  let target = directory.path().join("target");
  fs::write(&target, b"preserve").unwrap();
  let endpoint = directory.path().join("socket");
  symlink(&target, &endpoint).unwrap();
  assert!(Listener::bind(&Endpoint::Unix(endpoint), &ListenOptions::default()).await.is_err());
  let parent_link = directory.path().join("parent");
  symlink(directory.path(), &parent_link).unwrap();
  assert!(matches!(
    Listener::bind(&Endpoint::Unix(parent_link.join("new")), &ListenOptions::default()).await,
    Err(TransportError::AccessPolicy)
  ));
  assert_eq!(fs::read(target).unwrap(), b"preserve");
}

#[tokio::test]
async fn cleanup_preserves_replacement_regular_file() {
  let directory = private_dir();
  let path = directory.path().join("socket");
  let mut listener =
    Listener::bind(&Endpoint::Unix(path.clone()), &ListenOptions::default()).await.unwrap();
  fs::remove_file(&path).unwrap();
  fs::write(&path, b"replacement").unwrap();
  assert!(matches!(listener.cleanup(), Err(TransportError::AccessPolicy)));
  assert_eq!(fs::read(path).unwrap(), b"replacement");
}

#[tokio::test]
async fn cleanup_preserves_replacement_socket() {
  let directory = private_dir();
  let path = directory.path().join("socket");
  let mut listener =
    Listener::bind(&Endpoint::Unix(path.clone()), &ListenOptions::default()).await.unwrap();
  // Keep the old inode linked so it cannot be recycled for the replacement.
  fs::rename(&path, directory.path().join("old")).unwrap();
  let replacement = tokio::net::UnixListener::bind(&path).unwrap();
  assert!(matches!(listener.cleanup(), Err(TransportError::AccessPolicy)));
  assert!(path.exists());
  drop(replacement);
}

#[tokio::test]
async fn cleanup_checks_parent_identity_and_permissions() {
  let outer = private_dir();
  let parent = outer.path().join("private");
  fs::create_dir(&parent).unwrap();
  fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
  let path = parent.join("socket");
  let mut listener =
    Listener::bind(&Endpoint::Unix(path.clone()), &ListenOptions::default()).await.unwrap();
  fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
  assert!(matches!(listener.cleanup(), Err(TransportError::AccessPolicy)));
  fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
  fs::rename(&parent, outer.path().join("old-parent")).unwrap();
  fs::create_dir(&parent).unwrap();
  fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
  fs::write(&path, b"replacement").unwrap();
  assert!(matches!(listener.cleanup(), Err(TransportError::AccessPolicy)));
  assert_eq!(fs::read(&path).unwrap(), b"replacement");
  assert!(outer.path().join("old-parent/socket").exists());
}

#[tokio::test]
async fn dropping_listener_does_not_unlink_the_path() {
  let directory = private_dir();
  let path = directory.path().join("socket");
  let listener =
    Listener::bind(&Endpoint::Unix(path.clone()), &ListenOptions::default()).await.unwrap();
  drop(listener);
  assert!(path.exists());
}
