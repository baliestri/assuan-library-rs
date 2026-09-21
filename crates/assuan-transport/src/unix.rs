use std::{
  fs, io,
  os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
  path::{Path, PathBuf},
};

use crate::{Accepted, PeerIdentity, Stream, TransportError};

#[derive(Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
  device: u64,
  inode: u64,
}

impl FileIdentity {
  fn of(metadata: &fs::Metadata) -> Self {
    return Self {
      device: metadata.dev(),
      inode: metadata.ino(),
    };
  }
}

pub(crate) struct UnixListener {
  inner: Option<tokio::net::UnixListener>,
  path: PathBuf,
  parent: FileIdentity,
  socket: FileIdentity,
  uid: u32,
  cleaned: bool,
}

impl UnixListener {
  pub(crate) fn bind(path: &Path) -> Result<Self, TransportError> {
    let name = path.file_name().ok_or(TransportError::InvalidEndpoint)?;
    let supplied_parent =
      path.parent().filter(|value| return !value.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let uid = rustix::process::geteuid().as_raw();
    let original_parent = private_directory(supplied_parent, uid)?;
    let parent_path = fs::canonicalize(supplied_parent)?;
    let parent = private_directory(&parent_path, uid)?;
    if parent != original_parent {
      return Err(TransportError::AccessPolicy);
    }
    let path = parent_path.join(name);
    match fs::symlink_metadata(&path) {
      Ok(_) => return Err(io::Error::from(io::ErrorKind::AlreadyExists).into()),
      Err(error) if error.kind() == io::ErrorKind::NotFound => {}
      Err(error) => return Err(error.into()),
    }
    // The private directory prevents another OS user from racing the path.
    // Same-user processes and privileged processes are part of the trust
    // boundary.
    let inner = tokio::net::UnixListener::bind(&path)?;
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.file_type().is_socket() || metadata.uid() != uid {
      return Err(TransportError::AccessPolicy);
    }
    let mut listener = Self {
      inner: Some(inner),
      path,
      parent,
      socket: FileIdentity::of(&metadata),
      uid,
      cleaned: false,
    };
    if let Err(error) = fs::set_permissions(&listener.path, fs::Permissions::from_mode(0o600)) {
      let _ = listener.cleanup();
      return Err(error.into());
    }
    listener.verify_parent()?;
    return Ok(listener);
  }

  pub(crate) fn path(&self) -> &Path {
    return &self.path;
  }

  pub(crate) async fn accept(&self) -> Result<Accepted, TransportError> {
    let inner = self.inner.as_ref().ok_or(TransportError::InvalidEndpoint)?;
    let (stream, _) = inner.accept().await?;
    let credentials = stream.peer_cred()?;
    authorize_uid(credentials.uid(), self.uid)?;
    return Ok(Accepted {
      stream: Stream::new(stream),
      peer: Some(PeerIdentity::Unix {
        uid: credentials.uid(),
        gid: credentials.gid(),
        pid: credentials.pid().and_then(|pid| return u32::try_from(pid).ok()),
      }),
    });
  }

  fn verify_parent(&self) -> Result<(), TransportError> {
    let parent = self.path.parent().ok_or(TransportError::InvalidEndpoint)?;
    if private_directory(parent, self.uid)? != self.parent {
      return Err(TransportError::AccessPolicy);
    }
    return Ok(());
  }

  pub(crate) fn cleanup(&mut self) -> Result<(), TransportError> {
    if self.cleaned {
      return Ok(());
    }
    self.verify_parent()?;
    match fs::symlink_metadata(&self.path) {
      Ok(metadata) => {
        if !metadata.file_type().is_socket()
          || FileIdentity::of(&metadata) != self.socket
          || metadata.uid() != self.uid
        {
          return Err(TransportError::AccessPolicy);
        }
        fs::remove_file(&self.path)?;
      }
      Err(error) if error.kind() == io::ErrorKind::NotFound => {}
      Err(error) => return Err(error.into()),
    }
    self.inner = None;
    self.cleaned = true;
    return Ok(());
  }
}

fn private_directory(path: &Path, uid: u32) -> Result<FileIdentity, TransportError> {
  let metadata = fs::symlink_metadata(path)?;
  if !metadata.is_dir()
    || metadata.file_type().is_symlink()
    || metadata.uid() != uid
    || metadata.mode() & 0o077 != 0
  {
    return Err(TransportError::AccessPolicy);
  }
  return Ok(FileIdentity::of(&metadata));
}

fn authorize_uid(actual: u32, expected: u32) -> Result<(), TransportError> {
  if actual != expected {
    return Err(TransportError::AccessPolicy);
  }
  return Ok(());
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn different_peer_uid_is_rejected_including_when_server_is_root() {
    assert!(authorize_uid(1, 0).is_err());
    assert!(authorize_uid(0, 1).is_err());
    assert!(authorize_uid(42, 42).is_ok());
  }
}
