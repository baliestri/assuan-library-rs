use std::{fmt, future::Future, pin::Pin};

use crate::{Endpoint, ListenOptions, PeerIdentity, Stream, TransportError};

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

  /// Returns a standard transport address when the implementation has one.
  ///
  /// Defaults to `None`. Custom listeners need not invent an address or extend
  /// [`Endpoint`]. This optional metadata does not control connection
  /// acceptance.
  fn endpoint(&self) -> Option<&Endpoint> {
    return None;
  }
}

/// A standard transport listener. Dropping it closes its listening socket.
pub struct Listener {
  inner: Backend,
  endpoint: Endpoint,
}

enum Backend {
  Tcp(tokio::net::TcpListener),
  #[cfg(windows)]
  NamedPipe(crate::windows::PipeListener),
  #[cfg(unix)]
  Unix(crate::unix::UnixListener),
}

impl Listener {
  /// Borrows the actual bound address, including an OS-assigned TCP port.
  ///
  /// Standard listeners always have an endpoint. Through [`Acceptor`], the
  /// same address is returned as `Some`; custom listeners may return `None`.
  #[must_use]
  pub fn endpoint(&self) -> &Endpoint {
    return &self.endpoint;
  }

  /// Binds an explicitly selected endpoint in a Tokio runtime with I/O enabled.
  ///
  /// TCP does not apply the local-user policy: it has no OS peer credentials.
  /// Port zero is replaced by the actual bound port in [`Self::endpoint`].
  ///
  /// # Errors
  /// Returns [`TransportError::Io`] if binding or querying the socket fails, or
  /// [`TransportError::AccessPolicy`] if a Unix directory is not private and
  /// user-owned.
  ///
  /// # Panics
  /// Panics if called without a Tokio runtime with its I/O driver enabled.
  pub async fn bind(endpoint: &Endpoint, _options: &ListenOptions) -> Result<Self, TransportError> {
    match endpoint {
      #[cfg(windows)]
      Endpoint::NamedPipe(path) => {
        let inner = crate::windows::PipeListener::bind(path)?;
        return Ok(Self {
          inner: Backend::NamedPipe(inner),
          endpoint: endpoint.clone(),
        });
      }
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

  /// Explicitly removes a Unix socket only if its recorded identity still
  /// matches.
  ///
  /// Checks the private parent directory's owner, permissions, device and
  /// inode, and the socket's type, owner, device and inode. Success closes
  /// the listener; repeated cleanup is harmless. TCP and named-pipe cleanup
  /// are no-ops. Drop only closes the listener and never removes a pathname.
  /// Same-user and privileged processes must be trusted: pathname checks
  /// cannot prevent their concurrent mutations.
  ///
  /// # Errors
  /// Returns an access-policy error for changed identities or permissions, or
  /// an I/O error when inspection or removal fails. A replacement is never
  /// removed when detected. On failure the listener remains available for
  /// explicit handling.
  pub fn cleanup(&mut self) -> Result<(), TransportError> {
    match &mut self.inner {
      #[cfg(windows)]
      Backend::NamedPipe(_) => return Ok(()),
      Backend::Tcp(_) => return Ok(()),
      #[cfg(unix)]
      Backend::Unix(listener) => return listener.cleanup(),
    }
  }
}

impl Acceptor for Listener {
  /// Waits for a connection and validates local peer credentials before
  /// delivery.
  fn accept(&mut self) -> IoFuture<'_, Accepted> {
    return Box::pin(async move {
      match &self.inner {
        #[cfg(windows)]
        Backend::NamedPipe(listener) => return listener.accept().await,
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

  fn endpoint(&self) -> Option<&Endpoint> {
    return Some(&self.endpoint);
  }
}

impl fmt::Debug for Listener {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f.debug_struct("Listener").finish_non_exhaustive();
  }
}
