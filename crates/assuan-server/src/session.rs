use std::{fmt, sync::Arc};

use assuan_protocol::{
  Command, MAX_LINE_BYTES, RequestKind, Sensitivity, ServerLine, ServerMachine, ServerState,
  StateError, encode_response,
};
use assuan_transport::{Accepted, Channel, PeerIdentity, TransportError};
use tokio::time::{Instant, timeout_at};
use zeroize::Zeroizing;

use crate::{
  CommandContext, HandlerError, OptionRequest, Registry, ServerError, ServerOptions, SessionEnd,
  SessionHooks,
  error::{INTERNAL, INVALID_VALUE, UNKNOWN_COMMAND},
  inquiry::request_kind,
  options::deadline,
};

/// One accepted connection with exclusive typed state and a shared registry.
///
/// Commands borrow one fixed, wiped call buffer. Copying at most one wire line
/// separates command lifetime from the mutable channel used for responses and
/// inquiries. No unbounded request queue or data collection is performed.
///
/// Authentication runs once before greeting OK. The authentication flag and
/// transport identity are independent of application transient state.
pub struct Session<S: Send + 'static = ()> {
  channel: Channel,
  machine: ServerMachine,
  state: S,
  registry: Arc<Registry<S>>,
  hooks: Arc<dyn SessionHooks<S>>,
  options: ServerOptions,
  peer: Option<PeerIdentity>,
  authenticated: bool,
}

impl<S: Send + 'static> Session<S> {
  /// Takes ownership of a standard or custom accepted stream and its state.
  ///
  /// Options are validated when run is first polled, before authentication or
  /// wire output. Trust metadata only according to the acceptor's contract.
  #[must_use]
  pub fn new(
    accepted: Accepted,
    state: S,
    registry: Arc<Registry<S>>,
    hooks: Arc<dyn SessionHooks<S>>,
    options: ServerOptions,
  ) -> Self {
    return Self {
      channel: Channel::new(accepted.stream),
      machine: ServerMachine::new(),
      state,
      registry,
      hooks,
      options,
      peer: accepted.peer,
      authenticated: false,
    };
  }

  /// Authenticates and processes commands serially until disconnection or
  /// failure.
  ///
  /// Each hook/handler is bounded by one absolute operation deadline. Ordinary
  /// Remote and Internal command failures send one ERR and permit another
  /// command; invalid state, protocol failures and uncertain I/O close the
  /// stream. Internal failures expose only fixed generic text.
  ///
  /// The close hook runs after the channel closes, with a separate bounded
  /// budget. If both session and close hook fail, the session error is retained
  /// and only the close failure's category is logged through the log facade.
  /// Dropping this future or panicking closes the channel through ownership but
  /// cannot run asynchronous cleanup; no detached task is created.
  ///
  /// # Errors
  /// Returns invalid options, rejected authentication, transport/protocol/state
  /// failures, or close-hook failure after an otherwise successful session.
  ///
  /// # Panics
  /// Requires a Tokio runtime with time enabled. Application panics propagate.
  pub async fn run(mut self) -> Result<(), ServerError> {
    self.options.validate()?;
    let result = self.drive().await;
    self.channel.close();
    self.machine.close();
    let reason = result.as_ref().err().map_or(SessionEnd::Clean, end_reason);
    let close_deadline = deadline(self.options.shutdown_timeout)?;
    let closed = match timeout_at(close_deadline, self.hooks.closed(&mut self.state, reason)).await
    {
      Ok(result) => result.map_err(ServerError::Handler),
      Err(_) => Err(ServerError::Transport(TransportError::Timeout)),
    };
    if let Err(error) = closed {
      if result.is_ok() {
        return Err(error);
      }
      log::warn!(target: "assuan_server", "close hook failed: {:?}", end_reason(&error));
    }
    return result;
  }

  fn context(&mut self, deadline: Instant) -> CommandContext<'_, S> {
    return CommandContext {
      channel: &mut self.channel,
      machine: &mut self.machine,
      state: &mut self.state,
      deadline,
      options: &self.options,
      peer: self.peer.as_ref(),
      authenticated: self.authenticated,
    };
  }

  async fn drive(&mut self) -> Result<(), ServerError> {
    let greeting_deadline = deadline(self.options.greeting_timeout)?;
    let hooks = Arc::clone(&self.hooks);
    let auth = timeout_at(greeting_deadline, hooks.authenticate(self.context(greeting_deadline)))
      .await
      .map_err(|_| return ServerError::Transport(TransportError::Timeout))?;
    self.finalize(auth, greeting_deadline, true).await?;
    self.authenticated = true;
    loop {
      let idle_deadline = deadline(self.options.idle_timeout)?;
      let mut call = Box::new(Zeroizing::new([0; MAX_LINE_BYTES]));
      let len = loop {
        let Some(line) = self.channel.read_line_or_eof(idle_deadline, Sensitivity::Secret).await?
        else {
          return Ok(());
        };
        let kind = request_kind(line)?;
        self.machine.receive_request(kind)?;
        if matches!(kind, RequestKind::Empty | RequestKind::Comment) {
          continue;
        }
        call[..line.len()].copy_from_slice(line);
        break line.len();
      };
      let command = Command::parse(&call[..len])?;
      let closing = command.name() == "BYE";
      let command_deadline = deadline(self.options.command_timeout)?;
      let registry = Arc::clone(&self.registry);
      let hooks = Arc::clone(&self.hooks);
      let result = timeout_at(command_deadline, async {
        let context = self.context(command_deadline);
        match command.name() {
          "NOP" | "BYE" => return Ok(()),
          "HELP" => return crate::builtins::help(&registry, context).await,
          "OPTION" => {
            let request = OptionRequest::parse(command.args()).map_err(|_| {
              return HandlerError::Remote {
                code: INVALID_VALUE,
                message: "invalid option".into(),
              };
            })?;
            return hooks.option(request, context).await;
          }
          "RESET" => return hooks.reset(context).await,
          _ => {
            if let Some(handler) = registry.get(command.name()) {
              return handler.call(command, context).await;
            }
            return Err(HandlerError::Remote {
              code: UNKNOWN_COMMAND,
              message: "unknown command".into(),
            });
          }
        }
      })
      .await
      .map_err(|_| return ServerError::Transport(TransportError::Timeout))?;
      self.finalize(result, command_deadline, false).await?;
      if closing {
        return Ok(());
      }
    }
  }

  async fn finalize(
    &mut self,
    result: Result<(), HandlerError>,
    deadline: Instant,
    greeting: bool,
  ) -> Result<(), ServerError> {
    let result = match result {
      Err(
        error @ (HandlerError::Transport(_)
        | HandlerError::Protocol(_)
        | HandlerError::State(_)
        | HandlerError::Limit(_)),
      ) => {
        return Err(ServerError::Handler(error));
      }
      result => result,
    };
    if !matches!(
      self.machine.state(),
      ServerState::Greeting | ServerState::Handler | ServerState::AwaitFinal
    ) {
      return Err(ServerError::State(StateError::Unusable));
    }
    let line = match &result {
      Ok(()) => ServerLine::Ok(b""),
      Err(
        error @ HandlerError::Remote {
          code,
          message,
        },
      ) => {
        error.validate_remote()?;
        ServerLine::Err {
          code: *code,
          text: message.as_bytes(),
        }
      }
      Err(HandlerError::Internal) => {
        ServerLine::Err {
          code: INTERNAL,
          text: b"internal error",
        }
      }
      Err(_) => return Err(ServerError::Handler(result.err().unwrap_or(HandlerError::Internal))),
    };
    let kind = line.kind();
    let mut output = Box::new(Zeroizing::new([0; MAX_LINE_BYTES]));
    let len = encode_response(line, &mut output)?;
    self.machine.send_response(kind)?;
    self.channel.write_line(&output[..len], deadline, Sensitivity::Secret).await?;
    if greeting && let Err(error) = result {
      return Err(ServerError::Handler(error));
    }
    return Ok(());
  }
}

impl<S: Send + 'static> fmt::Debug for Session<S> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f
      .debug_struct("Session")
      .field("phase", &self.machine.state())
      .field("authenticated", &self.authenticated)
      .finish_non_exhaustive();
  }
}

pub(crate) fn end_reason(error: &ServerError) -> SessionEnd {
  return match error {
    ServerError::Transport(TransportError::Timeout)
    | ServerError::Handler(HandlerError::Transport(TransportError::Timeout)) => SessionEnd::Timeout,
    ServerError::Protocol(_)
    | ServerError::State(_)
    | ServerError::Transport(TransportError::Protocol(_))
    | ServerError::Handler(
      HandlerError::Protocol(_)
      | HandlerError::State(_)
      | HandlerError::Transport(TransportError::Protocol(_)),
    ) => SessionEnd::Protocol,
    ServerError::Transport(_) | ServerError::Handler(HandlerError::Transport(_)) => {
      SessionEnd::Transport
    }
    ServerError::Handler(_) | ServerError::InvalidOptions => SessionEnd::Handler,
  };
}
