use crate::{ClientError, ClientOptions, Transaction, options::deadline, session::SessionCore};
use assuan_protocol::{
  ClientMachine, ClientState, Command, MAX_LINE_BYTES, Sensitivity, StateError, encode_command,
};
use assuan_transport::{Channel, ConnectOptions, Endpoint, Stream};
use zeroize::Zeroizing;

/// One serial Assuan session with exclusive, borrowed command transactions.
///
/// Use separate connections for concurrency. A forgotten unfinished transaction
/// keeps the session busy; it cannot authorize another command.
#[derive(Debug)]
pub struct Client {
  pub(crate) core: SessionCore,
  options: ClientOptions,
}

impl Client {
  /// Connects through a standard transport and consumes its initial greeting.
  ///
  /// Native agent discovery handshakes belong to `ResolvedEndpoint::connect`;
  /// pass the resulting stream to [`Self::from_stream`].
  ///
  /// # Errors
  /// Returns invalid options, connection, greeting, or protocol errors.
  /// Cancelling this future drops any connection it owns.
  ///
  /// # Panics
  /// Requires a Tokio runtime with I/O and time enabled.
  pub async fn connect(endpoint: &Endpoint, options: ClientOptions) -> Result<Self, ClientError> {
    options.validate()?;
    let stream = assuan_transport::connect(
      endpoint,
      &ConnectOptions {
        timeout: options.connect_timeout,
      },
    )
    .await?;
    return Self::from_stream(stream, options).await;
  }

  /// Owns a standard or custom stream and waits for a successful greeting.
  ///
  /// Comments, empty lines, and status lines do not finish the greeting. An
  /// greeting inquiry is cancelled with CAN; its final response is still required.
  ///
  /// # Errors
  /// Returns typed timeout, I/O, greeting rejection, and malformed response errors.
  /// Invalid durations are rejected before I/O. Error or cancellation drops the stream.
  ///
  /// # Panics
  /// Requires a Tokio runtime with time enabled.
  pub async fn from_stream(stream: Stream, options: ClientOptions) -> Result<Self, ClientError> {
    return Self::handshake(stream, options).drive(None).await;
  }

  /// Starts an interactive greeting without performing I/O.
  ///
  /// The greeting deadline starts now. Invalid options are reported by the
  /// first next or finish call; they do not cause a panic or perform I/O.
  /// Dropping the returned handshake closes the owned stream.
  pub fn handshake(stream: Stream, options: ClientOptions) -> crate::Handshake {
    let validated = options.validate().and_then(|()| return deadline(options.greeting_timeout));
    let (deadline, error) = match validated {
      Ok(deadline) => (deadline, None),
      Err(error) => (tokio::time::Instant::now(), Some(error)),
    };
    let core = SessionCore {
      channel: Channel::new(stream),
      machine: ClientMachine::new(),
      io_uncertain: false,
      deadline,
      sensitivity: Sensitivity::Public,
      inquiry_timeout: options.inquiry_timeout,
      max_inquiry_bytes: options.max_inquiry_bytes,
    };
    return crate::Handshake {
      client: Some(Self {
        core,
        options,
      }),
      completion: crate::transaction::Completion::Pending,
      error,
    };
  }

  /// Connects and delegates greeting inquiries to a caller-provided handler.
  ///
  /// # Errors
  /// Returns option, connection, greeting, callback, or timeout errors. Callback
  /// failure or cancellation closes the owned connection. Each inquiry must be
  /// finished or cancelled; returning successfully without doing so is an error.
  ///
  /// # Panics
  /// Requires a Tokio runtime with I/O and time enabled.
  pub async fn connect_with(
    endpoint: &Endpoint,
    options: ClientOptions,
    handler: &mut dyn crate::GreetingHandler,
  ) -> Result<Self, ClientError> {
    options.validate()?;
    let stream = assuan_transport::connect(
      endpoint,
      &ConnectOptions {
        timeout: options.connect_timeout,
      },
    )
    .await?;
    return Self::handshake(stream, options).drive(Some(handler)).await;
  }
  /// Validates and sends a command, borrowing this session until completion.
  ///
  /// Arguments are already in wire representation. Encoding uses fixed protected
  /// heap storage before reserving the session or sending any bytes. Replies are
  /// classified as public; internal buffers are nevertheless always wiped.
  ///
  /// # Errors
  /// Local length errors leave a ready session reusable. Unfinished prior work,
  /// unsolicited buffered responses, failed writes, or cancelled writes prevent
  /// reuse. Cancellation after I/O starts leaves an uncertainty marker, even
  /// though no transaction was returned. A later operation closes that stream.
  ///
  /// # Panics
  /// Requires a Tokio runtime with time enabled.
  pub async fn command(&mut self, command: Command<'_>) -> Result<Transaction<'_>, ClientError> {
    return self.command_with(command, Sensitivity::Public).await;
  }

  /// Sends a command with explicit classification of all received payloads.
  ///
  /// Secret responses expose `SecretRef` instead of an unclassified byte slice.
  /// The classification is chosen before any command bytes are sent.
  ///
  /// # Errors
  /// Has the same validation, deadline, and cancellation behavior as command.
  ///
  /// # Panics
  /// Requires a Tokio runtime with time enabled.
  pub async fn command_with(
    &mut self,
    command: Command<'_>,
    sensitivity: Sensitivity,
  ) -> Result<Transaction<'_>, ClientError> {
    self.core.check()?;
    if self.core.machine.state() != ClientState::Ready {
      self.core.invalidate();
      return Err(StateError::NotReady.into());
    }
    let mut encoded = Box::new(Zeroizing::new([0; MAX_LINE_BYTES]));
    let len = encode_command(command, &mut encoded)?;
    self.core.deadline = deadline(self.options.command_timeout)?;
    // Responses already read ahead belong to the previous operation. Validate
    // them in Ready, before reserving or writing a new command.
    while self.core.channel.has_buffered_input() {
      self.core.read().await?;
    }
    self.core.sensitivity = sensitivity;
    self.core.machine.begin_command()?;
    self.core.write(&encoded[..len]).await?;
    return Ok(Transaction {
      core: &mut self.core,
      completion: crate::transaction::Completion::Pending,
    });
  }

  /// Reports readiness without I/O; uncertainty or unfinished work returns false.
  ///
  /// This is local session state, not a liveness probe of the remote peer.
  #[must_use]
  pub fn is_usable(&self) -> bool {
    return !self.core.io_uncertain && self.core.machine.state() == ClientState::Ready;
  }
}
