use super::{LineKind, RequestKind, StateError};

/// Current phase of a server session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerState {
  /// Greeting hooks are running before the initial OK.
  Greeting,
  /// Waiting for a client command.
  Ready,
  /// A handler owns the command and may emit its responses.
  Handler,
  /// Waiting for client data terminated by END or CAN.
  Inquiry,
  /// The client sent CAN; only informational lines and one final may follow.
  AwaitFinal,
  /// Protocol or I/O failure prevents reuse.
  Invalid,
  /// The connection is closed.
  Closed,
}

/// Pure server-side session validation, with exactly one final per operation.
///
/// Transport failures and abandoned operations must call [`Self::invalidate`].
/// This type handles neither I/O nor application dispatch.
#[derive(Debug)]
pub struct ServerMachine {
  state: ServerState,
  inquiry_origin: ServerState,
}

impl ServerMachine {
  /// Starts before the greeting's final response.
  #[must_use]
  pub const fn new() -> Self {
    return Self {
      state: ServerState::Greeting,
      inquiry_origin: ServerState::Greeting,
    };
  }

  /// Returns the current phase.
  #[must_use]
  pub const fn state(&self) -> ServerState {
    return self.state;
  }

  /// Applies a validated client request category.
  ///
  /// A new command is accepted only in Ready. Inquiry Data does not finish the
  /// inquiry; END resumes its origin, while CAN requires a final response.
  ///
  /// # Errors
  /// Returns [`StateError::UnexpectedEvent`] and invalidates on illegal input,
  /// or [`StateError::Unusable`] if already invalid/closed.
  pub fn receive_request(&mut self, event: RequestKind) -> Result<(), StateError> {
    use RequestKind as E;
    use ServerState as S;
    match (self.state, event) {
      (S::Invalid | S::Closed, _) => return Err(StateError::Unusable),
      (_, E::Comment | E::Empty) | (S::Inquiry, E::Data) => return Ok(()),
      (S::Ready, E::Command) => self.state = S::Handler,
      (S::Inquiry, E::End) => self.state = self.inquiry_origin,
      (S::Inquiry, E::Can) => self.state = S::AwaitFinal,
      _ => {
        self.invalidate();
        return Err(StateError::UnexpectedEvent);
      }
    }
    return Ok(());
  }

  /// Validates a response transition before the corresponding send.
  ///
  /// Status and comments do not finish an operation. Greeting ERR invalidates
  /// the session after accepting that final for sending. A handler's OK or ERR
  /// returns to Ready, where a duplicate final is rejected. If the send fails,
  /// the caller must invalidate even if this transition reached Ready.
  ///
  /// # Errors
  /// Returns [`StateError::UnexpectedEvent`] and invalidates on illegal output,
  /// or [`StateError::Unusable`] if already invalid/closed.
  pub fn send_response(&mut self, event: LineKind) -> Result<(), StateError> {
    use LineKind as E;
    use ServerState as S;
    match (self.state, event) {
      (S::Invalid | S::Closed, _) => return Err(StateError::Unusable),
      (_, E::Comment | E::Empty)
      | (S::Greeting | S::Handler | S::Inquiry | S::AwaitFinal, E::Status)
      | (S::Handler, E::Data | E::End) => return Ok(()),
      (S::Greeting | S::Handler, E::Inquire) => {
        self.inquiry_origin = self.state;
        self.state = S::Inquiry;
      }
      (S::Greeting, E::Ok) | (S::Handler, E::Ok | E::Err) => self.state = S::Ready,
      (S::Greeting, E::Err) => self.invalidate(),
      (S::AwaitFinal, E::Ok | E::Err) => {
        if self.inquiry_origin == S::Greeting && event == E::Err {
          self.invalidate();
        } else {
          self.state = S::Ready;
        }
      }
      _ => {
        self.invalidate();
        return Err(StateError::UnexpectedEvent);
      }
    }
    return Ok(());
  }

  /// Permanently forbids reuse after protocol failure or uncertain I/O.
  pub fn invalidate(&mut self) {
    if self.state != ServerState::Closed {
      self.state = ServerState::Invalid;
    }
  }

  /// Marks the connection closed; subsequent events cannot reopen it.
  pub fn close(&mut self) {
    self.state = ServerState::Closed;
  }
}

impl Default for ServerMachine {
  fn default() -> Self {
    return Self::new();
  }
}
