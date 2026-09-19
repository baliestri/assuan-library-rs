//! Discovery diagnostics and missing executable errors require no `GnuPG` install.

use assuan_transport::{AgentLocator, TransportError};

#[test]
fn locator_debug_omits_configuration_paths() {
  let locator = AgentLocator::new("private executable".into(), Some("private home".into()));
  assert!(!format!("{locator:?}").contains("private"));
}

#[tokio::test]
async fn missing_executable_returns_io_error() {
  let directory = tempfile::tempdir().unwrap();
  let locator = AgentLocator::new(directory.path().join("no-such-gpgconf"), None);
  assert!(matches!(locator.resolve().await, Err(TransportError::Io(_))));
}
