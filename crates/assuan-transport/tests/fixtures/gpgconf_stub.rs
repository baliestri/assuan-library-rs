//! Deterministic discovery subprocess, enabled only by the test-fixtures
//! feature.

use std::{
  fs,
  io::{self, Write},
  path::PathBuf,
  time::Duration,
};

fn main() {
  if run().is_err() {
    std::process::exit(2);
  }
}

fn run() -> io::Result<()> {
  let args: Vec<_> = std::env::args_os().skip(1).collect();
  if args.len() != 4
    || args[0] != "--homedir"
    || args[2] != "--list-dirs"
    || args[3] != "agent-socket"
  {
    return Err(io::ErrorKind::InvalidInput.into());
  }
  let home = PathBuf::from(&args[1]);
  let mode = fs::read_to_string(home.join("mode"))?;
  match mode.as_str() {
    "success" => io::stdout().write_all(&fs::read(home.join("output"))?)?,
    "failure" => {
      io::stderr().write_all(b"private subprocess diagnostics")?;
      std::process::exit(7);
    }
    "stderr" => {
      for _ in 0..257 {
        io::stderr().write_all(&[b'x'; 8192])?;
      }
    }
    "aggregate" => {
      std::thread::scope(|scope| {
        let stderr = scope.spawn(|| {
          for _ in 0..80 {
            io::stderr().write_all(&[b'e'; 8192])?;
          }
          return io::Result::Ok(());
        });
        for _ in 0..80 {
          io::stdout().write_all(&[b'o'; 8192])?;
        }
        return stderr.join().map_err(|_| return io::Error::other("worker failed"))?;
      })?;
    }
    "timeout" => {
      fs::write(home.join("pid"), std::process::id().to_string())?;
      std::thread::sleep(Duration::from_secs(30));
      fs::write(home.join("completed"), b"unexpected")?;
    }
    _ => return Err(io::ErrorKind::InvalidInput.into()),
  }
  return Ok(());
}
