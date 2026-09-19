use crate::{Endpoint, ListenOptions, PeerIdentity, Stream, TransportError};
use std::{fmt, future::Future, pin::Pin};

/// A borrowed, sendable transport operation returning a typed result.
pub type IoFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, TransportError>> + Send + 'a>>;

/// A newly accepted stream with optional OS-authenticated identity metadata.
#[derive(Debug)]
pub struct Accepted {
  /// Owned duplex byte stream.
  pub stream: Stream,
  /// OS identity for supported local IPC; always absent for TCP.
  pub peer: Option<PeerIdentity>,
}

/// Extensible asynchronous listener, independent of Assuan session processing.
///
/// Custom implementations must document cancellation behavior and must not
/// label an unauthenticated network address as an OS identity.
pub trait Acceptor: Send {
  /// Waits for one incoming connection.
  ///
  /// # Errors
  /// Returns typed I/O or access-policy failures from the implementation.
  fn accept(&mut self) -> IoFuture<'_, Accepted>;

  /// Returns the actual bound endpoint, including an OS-assigned TCP port.
  fn endpoint(&self) -> &Endpoint;
}

/// A standard transport listener. Dropping it closes its listening socket.
pub struct Listener {
  inner: Backend,
  endpoint: Endpoint,
}

enum Backend {
  Tcp(tokio::net::TcpListener),
  #[cfg(unix)]
  Unix(crate::unix::UnixListener),
}

impl Listener {
  /// Binds an explicitly selected endpoint in a Tokio runtime with I/O enabled.
  ///
  /// TCP does not apply the local-user policy: it has no OS peer credentials.
  /// Port zero is replaced by the actual bound port in [`Acceptor::endpoint`].
  ///
  /// # Errors
  /// Returns [`TransportError::Io`] if binding or querying the socket fails, or
  /// [`TransportError::AccessPolicy`] if a Unix directory is not private and user-owned.
  ///
  /// # Panics
  /// Panics if called without a Tokio runtime with its I/O driver enabled.
  pub async fn bind(endpoint: &Endpoint, _options: &ListenOptions) -> Result<Self, TransportError> {
    match endpoint {
      Endpoint::Tcp(address) => {
        let inner = tokio::net::TcpListener::bind(address).await?;
        let endpoint = Endpoint::Tcp(inner.local_addr()?);
        return Ok(Self {
          inner: Backend::Tcp(inner),
          endpoint,
        });
      }
      #[cfg(unix)]
      Endpoint::Unix(path) => {
        let inner = crate::unix::UnixListener::bind(path)?;
        let endpoint = Endpoint::Unix(inner.path().to_path_buf());
        return Ok(Self {
          inner: Backend::Unix(inner),
          endpoint,
        });
      }
    }
  }
  /// Explicitly removes a Unix socket only if its recorded identity still matches.
  ///
  /// Checks the private parent directory's owner, permissions, device and inode,
  /// and the socket's type, owner, device and inode. Success closes the listener;
  /// repeated cleanup is harmless. TCP cleanup is a no-op. Drop only closes the
  /// listener and never removes a pathname. Same-user and privileged processes
  /// must be trusted: pathname checks cannot prevent their concurrent mutations.
  ///
  /// # Errors
  /// Returns an access-policy error for changed identities or permissions, or an
  /// I/O error when inspection or removal fails. A replacement is never removed
  /// when detected. On failure the listener remains available for explicit handling.
  pub fn cleanup(&mut self) -> Result<(), TransportError> {
    match &mut self.inner {
      Backend::Tcp(_) => return Ok(()),
      #[cfg(unix)]
      Backend::Unix(listener) => return listener.cleanup(),
    }
  }
}

impl Acceptor for Listener {
  /// Waits for a connection and validates local peer credentials before delivery.
  fn accept(&mut self) -> IoFuture<'_, Accepted> {
    return Box::pin(async move {
      match &self.inner {
        Backend::Tcp(listener) => {
          let (stream, _) = listener.accept().await?;
          return Ok(Accepted {
            stream: Stream::new(stream),
            peer: None,
          });
        }
        #[cfg(unix)]
        Backend::Unix(listener) => return listener.accept().await,
      }
    });
  }

  fn endpoint(&self) -> &Endpoint {
    return &self.endpoint;
  }
}

impl fmt::Debug for Listener {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f.debug_struct("Listener").finish_non_exhaustive();
  }
}
