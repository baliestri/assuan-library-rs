use std::time::Duration;

use tokio::time::Instant;

use crate::ClientError;

/// Total operation deadlines for one serial client session.
#[derive(Debug, Clone)]
pub struct ClientOptions {
  /// Maximum duration of one inquiry, subordinate to the enclosing deadline;
  /// defaults to 120 seconds.
  pub inquiry_timeout: Duration,
  /// Maximum decoded bytes sent per inquiry; defaults to one MiB and must be
  /// positive.
  pub max_inquiry_bytes: usize,
  /// Transport connection timeout; defaults to ten seconds.
  pub connect_timeout: Duration,
  /// Total greeting timeout; defaults to 120 seconds.
  pub greeting_timeout: Duration,
  /// Total command timeout, including send and all responses; defaults to 300
  /// seconds.
  pub command_timeout: Duration,
}

impl Default for ClientOptions {
  fn default() -> Self {
    return Self {
      inquiry_timeout: Duration::from_secs(120),
      max_inquiry_bytes: 1024 * 1024,
      connect_timeout: Duration::from_secs(10),
      greeting_timeout: Duration::from_secs(120),
      command_timeout: Duration::from_secs(300),
    };
  }
}

impl ClientOptions {
  pub(crate) fn validate(&self) -> Result<(), ClientError> {
    if self.max_inquiry_bytes == 0 {
      return Err(ClientError::InvalidOptions);
    }
    for duration in
      [self.connect_timeout, self.greeting_timeout, self.command_timeout, self.inquiry_timeout]
    {
      deadline(duration)?;
    }
    return Ok(());
  }
}

pub(crate) fn deadline(duration: Duration) -> Result<Instant, ClientError> {
  if duration.is_zero() {
    return Err(ClientError::InvalidOptions);
  }
  return Instant::now().checked_add(duration).ok_or(ClientError::InvalidOptions);
}
