use std::fmt;

use assuan_protocol::{Command, PayloadRef, SecretBytes, SecretRef, Sensitivity};

use crate::{ClientError, Event, Transaction};

/// Finite retention limits for a collected response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollectLimits {
  max_bytes: usize,
  max_events: usize,
}

impl Default for CollectLimits {
  fn default() -> Self {
    return Self {
      max_bytes: 16 * 1024 * 1024,
      max_events: 4096,
    };
  }
}

impl CollectLimits {
  /// Creates positive byte and event limits.
  ///
  /// # Errors
  /// Returns [`ClientError::InvalidOptions`] when either limit is zero.
  pub fn new(max_bytes: usize, max_events: usize) -> Result<Self, ClientError> {
    if max_bytes == 0 || max_events == 0 {
      return Err(ClientError::InvalidOptions);
    }
    return Ok(Self {
      max_bytes,
      max_events,
    });
  }

  /// Returns the maximum retained bytes, including status and comment fields.
  #[must_use]
  pub const fn max_bytes(self) -> usize {
    return self.max_bytes;
  }

  /// Returns the maximum retained events.
  #[must_use]
  pub const fn max_events(self) -> usize {
    return self.max_events;
  }
}

/// A bounded public response collected from one completed transaction.
pub struct Response {
  data: Vec<u8>,
  statuses: Vec<(String, Vec<u8>)>,
  comments: Vec<Vec<u8>>,
}

impl Response {
  /// Returns concatenated decoded data lines.
  #[must_use]
  pub fn data(&self) -> &[u8] {
    return &self.data;
  }

  /// Returns retained status keyword and wire arguments.
  #[must_use]
  pub fn statuses(&self) -> &[(String, Vec<u8>)] {
    return &self.statuses;
  }

  /// Returns retained comment payloads.
  #[must_use]
  pub fn comments(&self) -> &[Vec<u8>] {
    return &self.comments;
  }
}

/// A bounded response whose data remains in protected storage.
pub struct SecretResponse {
  data: SecretBytes,
  statuses: Vec<(String, Vec<u8>)>,
  comments: Vec<Vec<u8>>,
}

impl SecretResponse {
  /// Explicitly exposes the collected secret data for the borrow duration.
  #[must_use]
  pub fn data(&self) -> SecretRef<'_> {
    return SecretRef::new(self.data.expose());
  }

  /// Returns retained status fields; callers must classify them independently.
  #[must_use]
  pub fn statuses(&self) -> &[(String, Vec<u8>)] {
    return &self.statuses;
  }

  /// Returns retained comments.
  #[must_use]
  pub fn comments(&self) -> &[Vec<u8>] {
    return &self.comments;
  }
}

impl crate::Client {
  /// Collects a bounded public response, cancelling inquiries automatically.
  ///
  /// Copies data, status fields, and comments into ordinary growable storage.
  /// Limits bound retained bytes and observed events, not allocator overhead.
  /// Returned views borrow the response. Default limits are 16 MiB and 4096
  /// events. Use streaming transactions to avoid retaining the whole response.
  /// Cancelling after I/O starts prevents session reuse; abandoning an active
  /// transaction closes it. A completed remote ERR preserves reuse. Inquiry
  /// CAN still requires the server's final response. No background drain runs.
  ///
  /// # Errors
  /// Returns protocol, transport, inquiry, or retention-limit failures.
  ///
  /// # Panics
  /// Requires a Tokio runtime with time enabled.
  pub async fn collect(
    &mut self,
    command: Command<'_>,
    limits: CollectLimits,
  ) -> Result<Response, ClientError> {
    let Collected::Public(response) =
      collect_transaction(self.command_with(command, Sensitivity::Public).await?, limits, false)
        .await?
    else {
      return Err(ClientError::Incomplete);
    };
    return Ok(response);
  }

  /// Collects a bounded response in protected storage, cancelling inquiries
  /// automatically.
  ///
  /// Allocates fixed protected data storage of `limits.max_bytes()` before
  /// receiving data. Data is copied directly from the borrowed receive buffer;
  /// status fields and comments remain ordinary owned memory. Do not use this
  /// collector when those metadata fields contain secrets; stream them instead.
  /// Returned views borrow the response. Retention limits and cancellation
  /// behavior are the same as [`Self::collect`].
  ///
  /// # Errors
  /// Returns protocol, transport, inquiry, or retention-limit failures.
  ///
  /// # Panics
  /// Requires a Tokio runtime with time enabled.
  pub async fn collect_secret(
    &mut self,
    command: Command<'_>,
    limits: CollectLimits,
  ) -> Result<SecretResponse, ClientError> {
    let Collected::Secret(response) =
      collect_transaction(self.command_with(command, Sensitivity::Secret).await?, limits, true)
        .await?
    else {
      return Err(ClientError::Incomplete);
    };
    return Ok(response);
  }
}

enum Collected {
  Public(Response),
  Secret(SecretResponse),
}

async fn collect_transaction(
  mut tx: Transaction<'_>,
  limits: CollectLimits,
  secret: bool,
) -> Result<Collected, ClientError> {
  let mut data = Vec::new();
  let mut secret_data = SecretBytes::with_capacity(if secret {
    limits.max_bytes
  } else {
    0
  })?;
  let mut statuses = Vec::new();
  let mut comments = Vec::new();
  let mut events = 0usize;
  let mut finished = false;
  loop {
    let event = tx.next().await?;
    let Some(event) = event else {
      break;
    };
    events = events
      .checked_add(1)
      .ok_or(ClientError::Limit(assuan_protocol::LimitError::LengthOverflow))?;
    if events > limits.max_events {
      return Err(ClientError::Limit(assuan_protocol::LimitError::CapacityExceeded));
    }
    match event {
      Event::Inquire(inquiry) => {
        inquiry.cancel().await?;
      }
      Event::Data(payload) => {
        let bytes = payload_slice(&payload);
        ensure_room(bytes.len(), &data, &secret_data, &statuses, &comments, limits.max_bytes)?;
        if secret {
          secret_data.extend_from_slice(bytes)?;
        } else {
          data.extend_from_slice(bytes);
        }
      }
      Event::Status {
        keyword,
        args,
      } => {
        let mut bytes = keyword
          .len()
          .checked_add(payload_len_ref(&args))
          .ok_or(ClientError::Limit(assuan_protocol::LimitError::LengthOverflow))?;
        bytes = bytes
          .checked_add(1)
          .ok_or(ClientError::Limit(assuan_protocol::LimitError::LengthOverflow))?;
        ensure_room(bytes, &data, &secret_data, &statuses, &comments, limits.max_bytes)?;
        statuses.push((keyword.to_owned(), payload_slice(&args).to_vec()));
      }
      Event::Comment(payload) => {
        ensure_room(
          payload_len_ref(&payload),
          &data,
          &secret_data,
          &statuses,
          &comments,
          limits.max_bytes,
        )?;
        comments.push(payload_slice(&payload).to_vec());
      }
      Event::End => {}
      Event::Finished {
        ..
      } => {
        finished = true;
        break;
      }
    }
  }
  if finished {
    tx.finish()?;
  }
  if secret {
    return Ok(Collected::Secret(SecretResponse {
      data: secret_data,
      statuses,
      comments,
    }));
  }
  return Ok(Collected::Public(Response {
    data,
    statuses,
    comments,
  }));
}

fn payload_slice<'a>(payload: &'a PayloadRef<'_>) -> &'a [u8] {
  return match payload {
    PayloadRef::Public(bytes) => bytes,
    PayloadRef::Secret(secret) => secret.expose(),
  };
}
fn payload_len_ref(payload: &PayloadRef<'_>) -> usize {
  return match payload {
    PayloadRef::Public(bytes) => bytes.len(),
    PayloadRef::Secret(secret) => secret.expose().len(),
  };
}
fn ensure_room(
  add: usize,
  data: &[u8],
  secret: &SecretBytes,
  statuses: &[(String, Vec<u8>)],
  comments: &[Vec<u8>],
  limit: usize,
) -> Result<(), ClientError> {
  let retained = data.len()
    + secret.len()
    + statuses.iter().map(|(k, v)| return k.len() + v.len() + 1).sum::<usize>()
    + comments.iter().map(Vec::len).sum::<usize>();
  if retained.checked_add(add).is_none_or(|n| return n > limit) {
    return Err(ClientError::Limit(assuan_protocol::LimitError::CapacityExceeded));
  }
  return Ok(());
}

impl fmt::Debug for Response {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f
      .debug_struct("Response")
      .field("data_bytes", &self.data.len())
      .field("status_count", &self.statuses.len())
      .field("comment_count", &self.comments.len())
      .finish_non_exhaustive();
  }
}

impl fmt::Debug for SecretResponse {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f
      .debug_struct("SecretResponse")
      .field("data_bytes", &self.data.len())
      .field("status_count", &self.statuses.len())
      .field("comment_count", &self.comments.len())
      .finish_non_exhaustive();
  }
}
