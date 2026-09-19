use crate::{ConnectOptions, Endpoint, Stream, TransportError};
use std::future::Future;
use tokio::time::{Instant, timeout_at};

/// Connects to an endpoint within the configured total deadline.
///
/// This performs no Assuan greeting or application authentication. Dropping the
/// future cancels the attempt and drops any stream it created. No detached task
/// is spawned. Use a Tokio runtime with I/O and time drivers enabled.
///
/// # Errors
/// Returns [`TransportError::Timeout`] on deadline expiry,
/// [`TransportError::InvalidOptions`] for an unrepresentable deadline,
/// [`TransportError::InvalidEndpoint`] for TCP port zero or an unspecified
/// destination address, or [`TransportError::Io`] on connection failure.
///
/// # Panics
/// Panics without a Tokio runtime with I/O and time enabled.
pub async fn connect(
  endpoint: &Endpoint,
  options: &ConnectOptions,
) -> Result<Stream, TransportError> {
  let Endpoint::Tcp(address) = endpoint;
  if address.port() == 0 || address.ip().is_unspecified() {
    return Err(TransportError::InvalidEndpoint);
  }
  return within_deadline(options, async {
    let stream = tokio::net::TcpStream::connect(address).await?;
    return Ok(Stream::new(stream));
  })
  .await;
}

async fn within_deadline<T>(
  options: &ConnectOptions,
  operation: impl Future<Output = Result<T, TransportError>>,
) -> Result<T, TransportError> {
  if options.timeout.is_zero() {
    return Err(TransportError::Timeout);
  }
  let deadline =
    Instant::now().checked_add(options.timeout).ok_or(TransportError::InvalidOptions)?;
  return timeout_at(deadline, operation).await.map_err(|_| return TransportError::Timeout)?;
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::{future::pending, time::Duration};

  #[tokio::test(start_paused = true)]
  async fn pending_attempt_expires_at_total_deadline() {
    let start = Instant::now();
    let result = within_deadline::<()>(&ConnectOptions::default(), pending()).await;
    assert!(matches!(result, Err(TransportError::Timeout)));
    assert_eq!(Instant::now() - start, Duration::from_secs(10));
  }

  #[tokio::test]
  async fn deadline_overflow_is_an_error_not_a_panic() {
    let result = within_deadline::<()>(
      &ConnectOptions {
        timeout: Duration::MAX,
      },
      pending(),
    )
    .await;
    assert!(matches!(result, Err(TransportError::InvalidOptions)));
  }
}
