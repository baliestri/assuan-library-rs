use std::time::Duration;

use tokio::time::Instant;

use crate::ServerError;

/// Positive limits for session operations, validated before authentication.
#[derive(Debug, Clone)]
pub struct ServerOptions {
  /// Total authentication and greeting budget; defaults to 120 seconds.
  pub greeting_timeout: Duration,
  /// Total command budget including handler work and final output; defaults to
  /// 300 seconds.
  pub command_timeout: Duration,
  /// Inquiry budget, capped by the enclosing deadline; defaults to 120 seconds.
  pub inquiry_timeout: Duration,
  /// Maximum decoded data bytes per inquiry; defaults to one MiB.
  pub max_inquiry_bytes: usize,
  /// Time between commands, including comments and partial lines; defaults to
  /// 300 seconds.
  pub idle_timeout: Duration,
  /// Maximum close-hook duration; defaults to 30 seconds.
  pub shutdown_timeout: Duration,
}

impl Default for ServerOptions {
  fn default() -> Self {
    return Self {
      greeting_timeout: Duration::from_secs(120),
      command_timeout: Duration::from_secs(300),
      inquiry_timeout: Duration::from_secs(120),
      max_inquiry_bytes: 1024 * 1024,
      idle_timeout: Duration::from_secs(300),
      shutdown_timeout: Duration::from_secs(30),
    };
  }
}

impl ServerOptions {
  pub(crate) fn validate(&self) -> Result<(), ServerError> {
    if self.max_inquiry_bytes == 0 {
      return Err(ServerError::InvalidOptions);
    }
    for duration in [
      self.greeting_timeout,
      self.command_timeout,
      self.inquiry_timeout,
      self.idle_timeout,
      self.shutdown_timeout,
    ] {
      deadline(duration)?;
    }
    return Ok(());
  }
}

pub(crate) fn deadline(duration: Duration) -> Result<Instant, ServerError> {
  if duration.is_zero() {
    return Err(ServerError::InvalidOptions);
  }
  return Instant::now().checked_add(duration).ok_or(ServerError::InvalidOptions);
}
