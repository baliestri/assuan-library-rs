use crate::{
  Client, ClientError, Event, Inquiry,
  transaction::{Completion, next_event},
};
use assuan_transport::TransportError;
use std::{future::Future, pin::Pin};
use tokio::time::timeout_at;

/// A sendable callback future borrowing the handler and inquiry for one response.
pub type ClientFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ClientError>> + Send + 'a>>;

/// Answers greeting inquiries without requiring a manually driven handshake.
///
/// Complete each inquiry with finish or cancel. An error, abandonment, or a
/// callback exceeding the greeting deadline closes the connection. Callback
/// errors are returned to the caller without incorporating metadata in messages.
pub trait GreetingHandler: Send {
  /// Handles one exclusive inquiry within the total greeting deadline.
  fn respond<'a>(&'a mut self, inquiry: Inquiry<'a>) -> ClientFuture<'a, ()>;
}

/// An owned, interactive greeting; dropping it closes the connection.
///
/// Created by `Client::handshake` without I/O. next exposes greeting status,
/// comments, inquiries, and the successful final response. finish transfers the
/// connected client only after completion; it never performs I/O.
#[derive(Debug)]
#[must_use = "drive the greeting to completion and call finish"]
pub struct Handshake {
  pub(crate) client: Option<Client>,
  pub(crate) completion: Completion,
  pub(crate) error: Option<ClientError>,
}

impl Handshake {
  /// Reads the next greeting event under the original greeting deadline.
  ///
  /// # Errors
  /// Reports invalid options deferred from construction, rejected greeting,
  /// malformed responses, forgotten inquiries, timeout, or uncertain I/O.
  /// Cancelling a pending read prevents reuse; drop closes the owned stream.
  ///
  /// # Panics
  /// Requires a Tokio runtime with time enabled.
  pub async fn next(&mut self) -> Result<Option<Event<'_>>, ClientError> {
    if let Some(error) = self.error.take() {
      if let Some(client) = self.client.as_mut() {
        client.core.invalidate();
      }
      return Err(error);
    }
    let client = self.client.as_mut().ok_or(ClientError::Incomplete)?;
    return next_event(&mut client.core, &mut self.completion).await;
  }

  /// Transfers the successfully greeted client without performing I/O.
  ///
  /// # Errors
  /// Rejects invalid options, unfinished greeting, rejected greeting, and
  /// uncertain I/O. Failure drops the owned connection.
  pub fn finish(mut self) -> Result<Client, ClientError> {
    if let Some(error) = self.error.take() {
      return Err(error);
    }
    let client = self.client.as_mut().ok_or(ClientError::Incomplete)?;
    client.core.check()?;
    match self.completion {
      Completion::Success => return self.client.take().ok_or(ClientError::Incomplete),
      Completion::Remote(code) => {
        return Err(ClientError::GreetingRejected {
          code,
        });
      }
      Completion::Pending => return Err(ClientError::Incomplete),
    }
  }

  pub(crate) async fn drive(
    mut self,
    mut handler: Option<&mut dyn GreetingHandler>,
  ) -> Result<Client, ClientError> {
    loop {
      let Some(event) = self.next().await? else {
        break;
      };
      if let Event::Inquire(inquiry) = event {
        let deadline = inquiry.deadline();
        match handler.as_deref_mut() {
          Some(handler) => {
            timeout_at(deadline, handler.respond(inquiry))
              .await
              .map_err(|_| return TransportError::Timeout)??;
          }
          None => inquiry.cancel().await?,
        }
      }
    }
    return self.finish();
  }
}
