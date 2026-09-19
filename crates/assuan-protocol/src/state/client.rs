use super::{LineKind, StateError};

/// Current phase of a client session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientState {
  /// Waiting for the initial successful greeting.
  Greeting,
  /// Available for one new command.
  Ready,
  /// A command is awaiting its final response.
  Command,
  /// The client must finish or cancel an inquiry.
  Inquiry,
  /// CAN was sent; a final OK or ERR is still required.
  AwaitFinal,
  /// Protocol or I/O state is uncertain; reuse is forbidden.
  Invalid,
  /// The connection was explicitly closed.
  Closed,
}

/// Pure client-side transition validation, independent of I/O and timeouts.
///
/// Call transitions only for validated lines and completed sends. A transport
/// or cancellation error must call [`Self::invalidate`]. Ignoring or forgetting
/// an unfinished transaction cannot make this machine ready for another one.
#[derive(Debug)]
pub struct ClientMachine {
  state: ClientState,
  inquiry_origin: ClientState,
}

impl ClientMachine {
  /// Starts a session awaiting its greeting.
  #[must_use]
  pub const fn new() -> Self {
    return Self {
      state: ClientState::Greeting,
      inquiry_origin: ClientState::Greeting,
    };
  }

  /// Returns the current phase without changing it.
  #[must_use]
  pub const fn state(&self) -> ClientState {
    return self.state;
  }

  /// Reserves the ready session for exactly one command.
  ///
  /// # Errors
  /// Returns [`StateError::NotReady`] outside Ready, or [`StateError::Unusable`]
  /// after invalidation/closure. A rejected local request preserves state.
  pub fn begin_command(&mut self) -> Result<(), StateError> {
    match self.state {
      ClientState::Ready => {
        self.state = ClientState::Command;
        return Ok(());
      }
      ClientState::Invalid | ClientState::Closed => return Err(StateError::Unusable),
      _ => return Err(StateError::NotReady),
    }
  }

  /// Applies a validated server response category.
  ///
  /// Comments and empty lines are ignored in live phases. Status is allowed
  /// during greeting, command, inquiry, and cancellation drain. Data and partial
  /// END are allowed only during a command. A final ERR completes a command,
  /// but rejects a greeting. An inquiry remembers its originating phase.
  ///
  /// # Errors
  /// Returns [`StateError::UnexpectedEvent`] and invalidates on an illegal
  /// event, [`StateError::GreetingRejected`] for greeting ERR, or
  /// [`StateError::Unusable`] if already invalid/closed.
  pub fn receive(&mut self, event: LineKind) -> Result<(), StateError> {
    use ClientState as S;
    use LineKind as E;
    match (self.state, event) {
      (S::Invalid | S::Closed, _) => return Err(StateError::Unusable),
      (_, E::Comment | E::Empty)
      | (S::Greeting | S::Command | S::Inquiry | S::AwaitFinal, E::Status)
      | (S::Command, E::Data | E::End) => return Ok(()),
      (S::Greeting | S::Command, E::Inquire) => {
        self.inquiry_origin = self.state;
        self.state = S::Inquiry;
      }
      (S::Greeting, E::Ok) | (S::Command, E::Ok | E::Err) => self.state = S::Ready,
      (S::Greeting, E::Err) => {
        self.invalidate();
        return Err(StateError::GreetingRejected);
      }
      (S::AwaitFinal, E::Ok | E::Err) => {
        if self.inquiry_origin == S::Greeting && event == E::Err {
          self.invalidate();
          return Err(StateError::GreetingRejected);
        }
        self.state = S::Ready;
      }
      _ => {
        self.invalidate();
        return Err(StateError::UnexpectedEvent);
      }
    }
    return Ok(());
  }

  /// Records a successfully sent END (`false`) or CAN (`true`) for an inquiry.
  ///
  /// END resumes the originating greeting or command. CAN enters `AwaitFinal`;
  /// it does not finish the enclosing operation or release the connection.
  ///
  /// # Errors
  /// Returns [`StateError::NotReady`] outside Inquiry or [`StateError::Unusable`]
  /// after invalidation/closure, without changing state.
  pub fn finish_inquiry(&mut self, cancelled: bool) -> Result<(), StateError> {
    match self.state {
      ClientState::Invalid | ClientState::Closed => return Err(StateError::Unusable),
      ClientState::Inquiry => {
        self.state = if cancelled {
          ClientState::AwaitFinal
        } else {
          self.inquiry_origin
        };
        return Ok(());
      }
      _ => return Err(StateError::NotReady),
    }
  }

  /// Permanently forbids reuse after uncertain I/O, abandonment, or failure.
  pub fn invalidate(&mut self) {
    if self.state != ClientState::Closed {
      self.state = ClientState::Invalid;
    }
  }

  /// Marks the connection closed; no subsequent event can reopen it.
  pub fn close(&mut self) {
    self.state = ClientState::Closed;
  }
}

impl Default for ClientMachine {
  fn default() -> Self {
    return Self::new();
  }
}
