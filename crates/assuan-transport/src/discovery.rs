use std::{
  fmt,
  path::PathBuf,
  process::Stdio,
  sync::atomic::{AtomicUsize, Ordering},
  time::Duration,
};

use assuan_protocol::{LimitError, SecretBytes};
use tokio::{
  io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
  process::Command,
  time::{Instant, timeout_at},
};
use zeroize::Zeroizing;

use crate::{ConnectOptions, DiscoveryError, Endpoint, Stream, TransportError};

const OUTPUT_LIMIT: usize = 1024 * 1024;
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);

/// Discovers the agent endpoint using an explicitly selected gpgconf
/// executable.
///
/// Invokes `--list-dirs agent-socket` without a shell, optionally preceded by
/// `--homedir`. This named form returns an unescaped pathname: percent signs
/// are literal. The process has a five-second total deadline and a combined
/// one-MiB stdout/stderr budget. Debug omits executable and directory paths.
pub struct AgentLocator {
  executable: PathBuf,
  homedir: Option<PathBuf>,
}

impl AgentLocator {
  /// Selects the executable and optional `GnuPG` home directory without
  /// spawning it.
  #[must_use]
  pub fn new(executable: PathBuf, homedir: Option<PathBuf>) -> Self {
    return Self {
      executable,
      homedir,
    };
  }

  /// Resolves a Unix socket or a native Windows loopback endpoint and nonce.
  ///
  /// Output is drained concurrently. Failed or timed-out child processes are
  /// killed and explicitly waited for; dropping this future uses Tokio's
  /// kill-on-drop behavior. No agent is launched and no greeting is consumed.
  /// Windows file reads use a bounded protected buffer on a blocking worker;
  /// an in-progress OS file read cannot be forcibly cancelled by Tokio.
  ///
  /// # Errors
  /// Returns typed I/O, discovery, allocation, or timeout errors. Captured
  /// output and nonce bytes are never included in diagnostics. Windows paths
  /// must be valid UTF-8; Unix path bytes are preserved without text
  /// conversion.
  ///
  /// # Panics
  /// Requires a Tokio runtime with I/O and time drivers enabled.
  pub async fn resolve(&self) -> Result<ResolvedEndpoint, TransportError> {
    let deadline = Instant::now() + DISCOVERY_TIMEOUT;
    let output = self.run_process(deadline).await?;
    let path = output_path(output.expose())?;
    #[cfg(unix)]
    {
      return Ok(ResolvedEndpoint {
        endpoint: Endpoint::Unix(path),
        nonce: None,
      });
    }
    #[cfg(windows)]
    {
      let task = tokio::task::spawn_blocking(move || return read_native_socket_file(&path));
      return timeout_at(deadline, task)
        .await
        .map_err(|_| return TransportError::Timeout)?
        .map_err(|_| return TransportError::Discovery(DiscoveryError::InvalidOutput))?;
    }
  }

  async fn run_process(&self, deadline: Instant) -> Result<SecretBytes, TransportError> {
    let mut command = Command::new(&self.executable);
    if let Some(home) = &self.homedir {
      command.arg("--homedir").arg(home);
    }
    command
      .args(["--list-dirs", "agent-socket"])
      .stdin(Stdio::null())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().ok_or(DiscoveryError::ProcessFailed)?;
    let stderr = child.stderr.take().ok_or(DiscoveryError::ProcessFailed)?;
    let budget = AtomicUsize::new(OUTPUT_LIMIT);
    let result = timeout_at(deadline, async {
      let (output, _, status) = tokio::try_join!(
        read_output(stdout, &budget, true),
        read_output(stderr, &budget, false),
        async {
          return child.wait().await.map_err(TransportError::from);
        }
      )?;
      if !status.success() {
        return Err(DiscoveryError::ProcessFailed.into());
      }
      return Ok(output);
    })
    .await
    .unwrap_or(Err(TransportError::Timeout));
    if result.is_err() {
      let _ = child.start_kill();
      let _ = child.wait().await;
    }
    return result;
  }
}

impl fmt::Debug for AgentLocator {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f
      .debug_struct("AgentLocator")
      .field("has_homedir", &self.homedir.is_some())
      .finish_non_exhaustive();
  }
}

/// A discovered endpoint with an optional protected native Windows nonce.
///
/// The nonce is private, not clonable, and wiped on normal drop. Connect does
/// not read the greeting: it exposes the stream only after writing the nonce.
pub struct ResolvedEndpoint {
  endpoint: Endpoint,
  nonce: Option<SecretBytes>,
}

impl ResolvedEndpoint {
  /// Borrows the transport address without exposing handshake credentials.
  #[must_use]
  pub fn endpoint(&self) -> &Endpoint {
    return &self.endpoint;
  }

  /// Connects and sends any native nonce under one total connection deadline.
  ///
  /// No Assuan response is read. A failed or cancelled attempt drops its
  /// stream.
  ///
  /// # Errors
  /// Returns connection, I/O, timeout, or invalid-option errors. Nonce bytes
  /// are not included in error messages. Zero duration immediately times out.
  ///
  /// # Panics
  /// Requires a Tokio runtime with I/O and time enabled.
  pub async fn connect(&self, options: &ConnectOptions) -> Result<Stream, TransportError> {
    if options.timeout.is_zero() {
      return Err(TransportError::Timeout);
    }
    let deadline =
      Instant::now().checked_add(options.timeout).ok_or(TransportError::InvalidOptions)?;
    return timeout_at(deadline, async {
      let mut stream = crate::connect(&self.endpoint, options).await?;
      if let Some(nonce) = &self.nonce {
        stream.write_all(nonce.expose()).await?;
        stream.flush().await?;
      }
      return Ok(stream);
    })
    .await
    .map_err(|_| return TransportError::Timeout)?;
  }
}

impl fmt::Debug for ResolvedEndpoint {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f
      .debug_struct("ResolvedEndpoint")
      .field("has_nonce", &self.nonce.is_some())
      .finish_non_exhaustive();
  }
}

async fn read_output(
  mut pipe: impl AsyncRead + Unpin,
  budget: &AtomicUsize,
  collect: bool,
) -> Result<SecretBytes, TransportError> {
  let mut output = SecretBytes::with_capacity(if collect {
    OUTPUT_LIMIT
  } else {
    0
  })?;
  let mut scratch = protected_buffer(8192)?;
  loop {
    let read = pipe.read(&mut scratch).await?;
    if read == 0 {
      return Ok(output);
    }
    budget
      .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
        return remaining.checked_sub(read);
      })
      .map_err(|_| return DiscoveryError::OutputLimit)?;
    if collect {
      output.extend_from_slice(&scratch[..read])?;
    }
  }
}

fn protected_buffer(capacity: usize) -> Result<Zeroizing<Box<[u8]>>, LimitError> {
  let mut bytes = Vec::new();
  bytes.try_reserve_exact(capacity).map_err(|_| return LimitError::AllocationFailed)?;
  bytes.resize(capacity, 0);
  return Ok(Zeroizing::new(bytes.into_boxed_slice()));
}

fn output_path(output: &[u8]) -> Result<PathBuf, TransportError> {
  let bytes = output.strip_suffix(b"\n").ok_or(DiscoveryError::InvalidOutput)?;
  #[cfg(windows)]
  let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
  if bytes.is_empty() || bytes.iter().any(|byte| return matches!(byte, 0 | b'\n')) {
    return Err(DiscoveryError::InvalidOutput.into());
  }
  #[cfg(unix)]
  let path = {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
  };
  #[cfg(windows)]
  let path =
    PathBuf::from(std::str::from_utf8(bytes).map_err(|_| return DiscoveryError::InvalidOutput)?);
  if !path.is_absolute() {
    return Err(DiscoveryError::InvalidOutput.into());
  }
  return Ok(path);
}

#[cfg(windows)]
fn read_native_socket_file(path: &std::path::Path) -> Result<ResolvedEndpoint, TransportError> {
  use std::io::Read;
  let mut file = std::fs::File::open(path)?;
  if !file.metadata()?.is_file() {
    return Err(DiscoveryError::InvalidOutput.into());
  }
  // One byte beyond the largest valid native file detects trailing data.
  let mut bytes = protected_buffer(23)?;
  let mut used = 0;
  while used < bytes.len() {
    let read = file.read(&mut bytes[used..])?;
    if read == 0 {
      break;
    }
    used += read;
  }
  return parse_native_socket_file(&bytes[..used]);
}

#[cfg(any(windows, test))]
fn parse_native_socket_file(bytes: &[u8]) -> Result<ResolvedEndpoint, TransportError> {
  if bytes.starts_with(b"!<socket >") {
    return Err(DiscoveryError::UnsupportedFormat.into());
  }
  if bytes.len() > 22 {
    return Err(DiscoveryError::InvalidOutput.into());
  }
  let split =
    bytes.iter().position(|byte| return *byte == b'\n').ok_or(DiscoveryError::InvalidOutput)?;
  if split == 0 || split > 5 || bytes.len() - split - 1 != 16 {
    return Err(DiscoveryError::InvalidOutput.into());
  }
  let mut port = 0_u16;
  for digit in &bytes[..split] {
    if !digit.is_ascii_digit() {
      return Err(DiscoveryError::InvalidOutput.into());
    }
    port = port
      .checked_mul(10)
      .and_then(|value| return value.checked_add(u16::from(digit - b'0')))
      .ok_or(DiscoveryError::InvalidOutput)?;
  }
  if port == 0 {
    return Err(DiscoveryError::InvalidOutput.into());
  }
  let mut nonce = SecretBytes::with_capacity(16)?;
  nonce.extend_from_slice(&bytes[split + 1..])?;
  return Ok(ResolvedEndpoint {
    endpoint: Endpoint::Tcp((std::net::Ipv4Addr::LOCALHOST, port).into()),
    nonce: Some(nonce),
  });
}

#[cfg(test)]
mod tests {
  #[test]
  fn native_socket_nonce_is_binary() {
    let mut fixture = b"32123\n".to_vec();
    fixture.extend_from_slice(&[0, 10, 13, 255, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
    let endpoint = super::parse_native_socket_file(&fixture).unwrap();
    assert!(!format!("{endpoint:?}").contains("255"));
  }

  #[test]
  fn malformed_native_files_are_rejected() {
    for prefix in [
      b"0\n".as_slice(),
      b"65536\n",
      b"9999999999\n",
      b"-1\n",
      b"+1\n",
      b"1\r\n",
      b" 1\n",
      b"x\n",
      b"\n",
    ] {
      let fixture = [prefix, &[0; 16]].concat();
      assert!(super::parse_native_socket_file(&fixture).is_err());
    }
    for length in [0, 15, 17, 1024] {
      let fixture = [b"1\n".as_slice(), &vec![0; length]].concat();
      assert!(super::parse_native_socket_file(&fixture).is_err());
    }
    assert!(matches!(
      super::parse_native_socket_file(b"!<socket >123 s nonce"),
      Err(crate::TransportError::Discovery(crate::DiscoveryError::UnsupportedFormat))
    ));
    for port in [1, 65535] {
      let fixture = [format!("{port}\n").as_bytes(), &[0; 16]].concat();
      assert!(super::parse_native_socket_file(&fixture).is_ok());
    }
  }

  #[tokio::test]
  async fn native_handshake_precedes_greeting_without_consuming_it() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let nonce = [0, 10, 13, 255, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    let fixture =
      [format!("{}\n", listener.local_addr().unwrap().port()).as_bytes(), &nonce].concat();
    let resolved = super::parse_native_socket_file(&fixture).unwrap();
    let server = tokio::spawn(async move {
      let (mut stream, _) = listener.accept().await.unwrap();
      let mut received = [0; 16];
      stream.read_exact(&mut received).await.unwrap();
      assert_eq!(received, nonce);
      stream.write_all(b"OK\n").await.unwrap();
    });
    let mut stream = resolved.connect(&crate::ConnectOptions::default()).await.unwrap();
    let mut greeting = [0; 3];
    tokio::time::timeout(std::time::Duration::from_secs(2), stream.read_exact(&mut greeting))
      .await
      .unwrap()
      .unwrap();
    assert_eq!(&greeting, b"OK\n");
    server.await.unwrap();
  }
}
