//! Isolated `GnuPG` fixture; never uses the user's agent home.

use std::{
  error::Error,
  fmt, io,
  path::{Path, PathBuf},
  process::Stdio,
  time::{Duration, Instant},
};

use assuan_library::AgentLocator;
use tempfile::TempDir;
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  process::Command,
  time::timeout,
};

pub type TestError = Box<dyn Error + Send + Sync>;
const DEADLINE: Duration = Duration::from_secs(20);
const OUTPUT_LIMIT: u64 = 64 * 1024;

#[derive(Debug)]
struct MissingExecutable;

impl fmt::Display for MissingExecutable {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f.write_str("a required GnuPG executable was not found");
  }
}
impl Error for MissingExecutable {}

pub struct GnuPgFixture {
  home: Option<TempDir>,
  gpgconf: PathBuf,
  connector: PathBuf,
  socket: PathBuf,
  running: bool,
}

impl GnuPgFixture {
  pub async fn start() -> Result<Self, TestError> {
    let home = private_home().await?;
    let mut fixture = Self {
      home: Some(home),
      gpgconf: std::env::var_os("ASSUAN_GPGCONF").unwrap_or_else(|| return "gpgconf".into()).into(),
      connector: std::env::var_os("ASSUAN_GPG_CONNECT_AGENT")
        .unwrap_or_else(|| return "gpg-connect-agent".into())
        .into(),
      socket: PathBuf::new(),
      running: false,
    };
    for (label, executable) in
      [("gpgconf", &fixture.gpgconf), ("gpg-connect-agent", &fixture.connector)]
    {
      let output = run(fixture.command(executable).arg("--version"), b"").await.map_err(
        |error| -> TestError {
          if error
            .downcast_ref::<io::Error>()
            .is_some_and(|error| return error.kind() == io::ErrorKind::NotFound)
          {
            return Box::new(MissingExecutable);
          }
          return error;
        },
      )?;
      let version = std::str::from_utf8(&output)?
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .last()
        .unwrap_or("");
      if version.is_empty()
        || !version.bytes().all(|b| return b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
      {
        return Err(io::Error::other("invalid tool version output").into());
      }
      eprintln!("{label} version: {version}");
    }
    let output =
      run(fixture.command(&fixture.gpgconf).args(["--list-dirs", "agent-socket"]), b"").await?;
    fixture.socket = PathBuf::from(std::str::from_utf8(&output)?.trim_end_matches(['\r', '\n']));
    if !fixture.socket.is_absolute() {
      return Err(io::Error::other("agent socket must be absolute").into());
    }
    #[cfg(windows)]
    if fixture.socket.as_os_str().len() + ".browser".len() >= 108 {
      return Err(
        io::Error::other(
          "GnuPG socket path is too long; run scripts/test-interop.ps1 with a short tool alias",
        )
        .into(),
      );
    }
    // Set before launch so even a failed or cancelled launch attempts cleanup.
    fixture.running = true;
    run(fixture.command(&fixture.gpgconf).args(["--launch", "gpg-agent"]), b"").await?;
    let deadline = tokio::time::Instant::now() + DEADLINE;
    while !fixture.socket.try_exists()? {
      if tokio::time::Instant::now() >= deadline {
        return Err(io::Error::other("agent socket did not appear after launch").into());
      }
      tokio::time::sleep(Duration::from_millis(20)).await;
    }
    return Ok(fixture);
  }

  pub fn home(&self) -> &Path {
    return self.home.as_ref().expect("fixture home exists until shutdown").path();
  }

  #[allow(dead_code)] // Used by the agent test, not the separate server test.
  pub fn locator(&self) -> AgentLocator {
    return AgentLocator::new(self.gpgconf.clone(), Some(self.home().to_owned()));
  }

  fn command(&self, executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command.arg("--homedir").arg(self.home()).env("GNUPGHOME", self.home());
    return command;
  }

  // This support module is compiled independently into both integration tests.
  #[cfg(unix)]
  #[allow(dead_code)]
  pub async fn connect_raw(&self, socket: &Path, input: &[u8]) -> Result<Vec<u8>, TestError> {
    return run(
      self.command(&self.connector).arg("--no-autostart").arg("--raw-socket").arg(socket),
      input,
    )
    .await;
  }

  pub async fn shutdown(&mut self) -> Result<(), TestError> {
    if self.running {
      run(self.command(&self.gpgconf).args(["--kill", "gpg-agent"]), b"").await?;
      let deadline = tokio::time::Instant::now() + DEADLINE;
      while self.socket.try_exists()? {
        if tokio::time::Instant::now() >= deadline {
          return Err(io::Error::other("agent socket remained after shutdown").into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
      }
      self.running = false;
    }
    if let Some(home) = self.home.take() {
      home.close()?;
    }
    return Ok(());
  }
}

impl Drop for GnuPgFixture {
  fn drop(&mut self) {
    if !self.running {
      return;
    }
    // Emergency cleanup is synchronous and bounded, including during unwinding.
    // Never target the user's default home and never spawn a detached cleanup
    // task.
    let result = self.emergency_shutdown();
    if result.is_err() {
      eprintln!("GnuPG fixture emergency cleanup failed; isolated home retained");
      if let Some(home) = self.home.take() {
        let _ = home.keep();
      }
    }
  }
}

impl GnuPgFixture {
  fn emergency_shutdown(&self) -> io::Result<()> {
    let mut command = std::process::Command::new(&self.gpgconf);
    command
      .arg("--homedir")
      .arg(self.home())
      .args(["--kill", "gpg-agent"])
      .env("GNUPGHOME", self.home())
      .stdin(Stdio::null())
      .stdout(Stdio::null())
      .stderr(Stdio::null());
    #[cfg(windows)]
    {
      use std::os::windows::process::CommandExt;
      command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn()?;
    let deadline = Instant::now() + DEADLINE;
    loop {
      if let Some(status) = child.try_wait()? {
        if !status.success() {
          return Err(io::Error::other("agent cleanup command failed"));
        }
        break;
      }
      if Instant::now() >= deadline {
        let _ = child.kill();
        let _ = child.wait();
        return Err(io::Error::other("agent cleanup command timed out"));
      }
      std::thread::sleep(Duration::from_millis(20));
    }
    while self.socket.try_exists()? {
      if Instant::now() >= deadline {
        return Err(io::Error::other("agent cleanup timed out"));
      }
      std::thread::sleep(Duration::from_millis(20));
    }
    return Ok(());
  }
}

pub async fn start_or_skip() -> Result<Option<GnuPgFixture>, TestError> {
  match GnuPgFixture::start().await {
    Ok(fixture) => return Ok(Some(fixture)),
    Err(error)
      if error.is::<MissingExecutable>()
        && std::env::var_os("ASSUAN_GNUPG_REQUIRED").as_deref()
          != Some(std::ffi::OsStr::new("1")) =>
    {
      eprintln!(
        "SKIPPED GnuPG interoperability: executable missing (not an interoperability pass)"
      );
      return Ok(None);
    }
    Err(error) => return Err(error),
  }
}

#[cfg_attr(unix, allow(clippy::unused_async))] // Windows ACL setup is asynchronous.
pub async fn private_home() -> Result<TempDir, TestError> {
  let root = Path::new(env!("CARGO_TARGET_TMPDIR"));
  std::fs::create_dir_all(root)?;
  let home = tempfile::Builder::new().prefix("assuan-").tempdir_in(root)?;
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700))?;
  }
  #[cfg(windows)]
  {
    let mut command = Command::new("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-Command",
      "$ErrorActionPreference = 'Stop'; $sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User; $acl = [System.Security.AccessControl.DirectorySecurity]::new(); $acl.SetAccessRuleProtection($true, $false); $rule = [System.Security.AccessControl.FileSystemAccessRule]::new($sid, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow'); $acl.AddAccessRule($rule); [System.IO.Directory]::SetAccessControl($env:ASSUAN_TEST_HOME, $acl)"])
      .env("ASSUAN_TEST_HOME", home.path());
    run(&mut command, b"")
      .await
      .map_err(|_| return io::Error::other("private fixture ACL setup failed"))?;
  }
  return Ok(home);
}

async fn run(command: &mut Command, input: &[u8]) -> Result<Vec<u8>, TestError> {
  command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
  #[cfg(windows)]
  command.creation_flags(0x0800_0000);
  let mut child = command.spawn()?;
  let mut stdin =
    child.stdin.take().ok_or_else(|| return io::Error::other("missing process stdin"))?;
  let stdout =
    child.stdout.take().ok_or_else(|| return io::Error::other("missing process stdout"))?;
  let stderr =
    child.stderr.take().ok_or_else(|| return io::Error::other("missing process stderr"))?;
  let result = timeout(DEADLINE, async {
    let mut output = Vec::new();
    let mut errors = Vec::new();
    let ((), _, _, status) = tokio::try_join!(
      async move {
        stdin.write_all(input).await?;
        drop(stdin);
        return Ok::<(), io::Error>(());
      },
      async {
        return stdout.take(OUTPUT_LIMIT + 1).read_to_end(&mut output).await;
      },
      async {
        return stderr.take(OUTPUT_LIMIT + 1).read_to_end(&mut errors).await;
      },
      child.wait(),
    )?;
    if !status.success() {
      return Err(io::Error::other("GnuPG fixture process failed"));
    }
    if output.len() as u64 > OUTPUT_LIMIT || errors.len() as u64 > OUTPUT_LIMIT {
      return Err(io::Error::other("GnuPG fixture output limit exceeded"));
    }
    return Ok(output);
  })
  .await;
  match result {
    Ok(Ok(output)) => return Ok(output),
    failure => {
      let _ = child.start_kill();
      let _ = child.wait().await;
      return match failure {
        Ok(Err(error)) => Err(error.into()),
        Err(_) => Err(io::Error::other("GnuPG fixture process timed out").into()),
        Ok(Ok(_)) => unreachable!(),
      };
    }
  }
}
