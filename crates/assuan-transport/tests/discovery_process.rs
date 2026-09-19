//! Bounded subprocess discovery, using an explicitly selected fixture binary.
#![cfg(feature = "test-fixtures")]

use assuan_transport::{AgentLocator, DiscoveryError, Endpoint, TransportError};
use std::{
  fs,
  path::Path,
  time::{Duration, Instant},
};

fn fixture(mode: &str, output: &[u8]) -> (tempfile::TempDir, AgentLocator) {
  let home = tempfile::Builder::new().prefix("gpg home % ; ").tempdir().unwrap();
  fs::write(home.path().join("mode"), mode).unwrap();
  fs::write(home.path().join("output"), output).unwrap();
  let locator =
    AgentLocator::new(env!("CARGO_BIN_EXE_gpgconf_stub").into(), Some(home.path().to_path_buf()));
  return (home, locator);
}

fn path_output(path: &Path) -> Vec<u8> {
  #[cfg(unix)]
  let mut bytes = {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
  };
  #[cfg(windows)]
  let mut bytes = path.to_str().unwrap().as_bytes().to_vec();
  bytes.push(b'\n');
  return bytes;
}

#[tokio::test]
async fn selected_path_preserves_percent_sequences_and_spaces() {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("socket %20 literal");
  #[cfg(windows)]
  fs::write(&path, [b"32123\n".as_slice(), &[0xFF; 16]].concat()).unwrap();
  let (_home, locator) = fixture("success", &path_output(&path));
  let resolved = locator.resolve().await.unwrap();
  #[cfg(unix)]
  assert_eq!(resolved.endpoint(), &Endpoint::Unix(path));
  #[cfg(windows)]
  assert_eq!(resolved.endpoint(), &Endpoint::Tcp("127.0.0.1:32123".parse().unwrap()));
  assert!(!format!("{resolved:?}").contains("255"));
}

#[tokio::test]
async fn stdout_and_stderr_share_one_budget() {
  for mode in ["stderr", "aggregate"] {
    let (_home, locator) = fixture(mode, b"");
    assert!(
      matches!(
        locator.resolve().await,
        Err(TransportError::Discovery(DiscoveryError::OutputLimit))
      ),
      "{mode}"
    );
  }
}

#[tokio::test]
async fn unsuccessful_exit_is_not_parsed_as_an_endpoint() {
  let (_home, locator) = fixture("failure", b"");
  let error = locator.resolve().await.unwrap_err();
  assert!(matches!(error, TransportError::Discovery(DiscoveryError::ProcessFailed)));
  assert!(!format!("{error:?} {error}").contains("private"));
}

#[tokio::test]
async fn malformed_output_is_rejected_without_trimming_or_guessing() {
  for output in [b"".as_slice(), b"relative\n", b"/one\n/two\n", b"/nul\0path\n", b"/unterminated"]
  {
    let (_home, locator) = fixture("success", output);
    assert!(matches!(
      locator.resolve().await,
      Err(TransportError::Discovery(DiscoveryError::InvalidOutput))
    ));
  }
}

#[tokio::test]
async fn timeout_kills_and_reaps_the_subprocess() {
  let (home, locator) = fixture("timeout", b"");
  let started = Instant::now();
  assert!(matches!(locator.resolve().await, Err(TransportError::Timeout)));
  assert!(started.elapsed() >= Duration::from_secs(4));
  assert!(started.elapsed() < Duration::from_secs(15));
  assert!(!home.path().join("completed").exists());
  #[cfg(target_os = "linux")]
  {
    let pid = fs::read_to_string(home.path().join("pid")).unwrap();
    assert!(!Path::new("/proc").join(pid).exists(), "child was not reaped");
  }
}

#[cfg(unix)]
#[tokio::test]
async fn unix_non_utf8_path_bytes_are_preserved() {
  use std::{ffi::OsString, os::unix::ffi::OsStringExt};
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join(OsString::from_vec(b"socket\xff%20".to_vec()));
  let (_home, locator) = fixture("success", &path_output(&path));
  assert_eq!(locator.resolve().await.unwrap().endpoint(), &Endpoint::Unix(path));
}

#[cfg(windows)]
#[tokio::test]
async fn native_file_is_bounded_and_binary() {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("socket");
  for contents in
    [vec![b'x'; 1024], b"!<socket >32123 s xxxxxxxxxxxxxxxxx".to_vec(), b"1\nshort".to_vec()]
  {
    fs::write(&path, contents).unwrap();
    let (_home, locator) = fixture("success", &path_output(&path));
    assert!(matches!(locator.resolve().await, Err(TransportError::Discovery(_))));
  }
}
