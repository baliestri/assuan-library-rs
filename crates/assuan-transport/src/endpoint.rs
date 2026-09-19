use std::{net::SocketAddr, time::Duration};

/// An explicitly addressed transport endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Endpoint {
  /// A TCP address; binding port zero asks the OS to assign a port.
  ///
  /// TCP does not authenticate a local OS user. Bind loopback when remote
  /// exposure is unnecessary; application authentication is a separate layer.
  Tcp(SocketAddr),
  /// A filesystem Unix socket. Listeners require a private, user-owned parent directory.
  #[cfg(unix)]
  Unix(std::path::PathBuf),
}

/// Options for a complete transport connection attempt.
#[derive(Debug, Clone)]
pub struct ConnectOptions {
  /// Total connection deadline, defaulting to ten seconds.
  ///
  /// Zero rejects the attempt as timed out without opening a connection.
  /// An unrepresentable deadline returns `TransportError::InvalidOptions`.
  pub timeout: Duration,
}

impl Default for ConnectOptions {
  fn default() -> Self {
    return Self {
      timeout: Duration::from_secs(10),
    };
  }
}

/// Required access control for local IPC endpoints, not TCP sockets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum LocalAccess {
  /// Restrict the local endpoint to the current OS user or fail explicitly.
  #[default]
  CurrentUser,
}

/// Listener options; local IPC defaults to current-user-only access.
///
/// The policy applies to Unix sockets and named pipes as those backends become
/// available. It provides no authentication or user filtering for TCP.
#[derive(Debug, Clone, Default)]
pub struct ListenOptions {
  local_access: LocalAccess,
}

impl ListenOptions {
  /// Selects the required local IPC access policy.
  #[must_use]
  pub const fn with_local_access(mut self, access: LocalAccess) -> Self {
    self.local_access = access;
    return self;
  }

  /// Returns the policy applied to local IPC endpoints.
  #[must_use]
  pub const fn local_access(&self) -> LocalAccess {
    return self.local_access;
  }
}

/// OS-reported peer identity metadata, distinct from a network address.
///
/// TCP connections never populate this type. Custom acceptors are responsible
/// for the provenance of metadata they provide; this enum is not a capability.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PeerIdentity {
  /// Unix credentials returned by the operating system for a local peer.
  #[cfg(unix)]
  Unix {
    /// Effective user identifier.
    uid: u32,
    /// Effective group identifier.
    gid: u32,
    /// Process identifier when the OS supplies one.
    pid: Option<u32>,
  },
  /// Windows security identifier verified for a local peer.
  #[cfg(windows)]
  Windows {
    /// Canonical SID string obtained through OS identity validation.
    sid: String,
  },
}
