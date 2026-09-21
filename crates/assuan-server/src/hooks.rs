use std::{fmt, future::Future, pin::Pin};

use assuan_protocol::ProtocolError;

use crate::{CommandContext, HandlerError, error::UNKNOWN_OPTION};

/// An asynchronous hook borrowing its policy and session-local state.
pub type HookFuture<'a> = Pin<Box<dyn Future<Output = Result<(), HandlerError>> + Send + 'a>>;

/// Payload-free reason for normal close-hook notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionEnd {
  /// The peer disconnected between complete commands.
  Clean,
  /// Invalid wire data, incomplete inquiry, or an illegal state transition.
  Protocol,
  /// Transport I/O failed.
  Transport,
  /// Explicit cancellation by a session owner that can await cleanup.
  ///
  /// Dropping or aborting `Session::run` cannot invoke the asynchronous hook.
  Cancelled,
  /// A total operation deadline expired.
  Timeout,
  /// Authentication or application policy failed.
  Handler,
}

/// A validated OPTION name and optional value borrowed from the command.
///
/// Debug reveals only lengths. An absent value differs from an empty value.
pub struct OptionRequest<'a> {
  name: &'a [u8],
  value: Option<&'a [u8]>,
}

impl<'a> OptionRequest<'a> {
  /// Parses a name, optional leading `--`, and a value separated by space or
  /// `=`.
  ///
  /// Outer ASCII whitespace and spaces around the separator are ignored.
  /// Values remain bytes and are not percent-decoded.
  ///
  /// # Errors
  /// Rejects CR, LF, NUL, missing names, and invalid ASCII name tokens.
  pub fn parse(bytes: &'a [u8]) -> Result<Self, ProtocolError> {
    if bytes.iter().any(|b| return matches!(b, 0 | b'\r' | b'\n')) {
      return Err(ProtocolError::InvalidLine);
    }
    let bytes = trim(bytes);
    let bytes = bytes.strip_prefix(b"--").unwrap_or(bytes);
    let end =
      bytes.iter().position(|b| return matches!(b, b' ' | b'\t' | b'=')).unwrap_or(bytes.len());
    let name = &bytes[..end];
    let text = std::str::from_utf8(name).map_err(|_| return ProtocolError::InvalidToken)?;
    assuan_protocol::Command::new(text, b"")?;
    let rest = trim(&bytes[end..]);
    let value = if let Some(value) = rest.strip_prefix(b"=") {
      Some(trim(value))
    } else if rest.is_empty() {
      None
    } else {
      Some(rest)
    };
    return Ok(Self {
      name,
      value,
    });
  }

  /// Borrows the option name without the optional prefix.
  #[must_use]
  pub const fn name(&self) -> &'a [u8] {
    return self.name;
  }

  /// Borrows the value without percent decoding; None means no value was
  /// provided.
  #[must_use]
  pub const fn value(&self) -> Option<&'a [u8]> {
    return self.value;
  }
}

fn trim(bytes: &[u8]) -> &[u8] {
  return bytes.trim_ascii();
}

impl fmt::Debug for OptionRequest<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f
      .debug_struct("OptionRequest")
      .field("name_len", &self.name.len())
      .field("value_len", &self.value.map(<[u8]>::len))
      .finish();
  }
}

/// Policy shared by sessions while each invocation borrows one session's state.
///
/// Hooks may issue inquiries. Only the runner sends final responses. RESET
/// must preserve application authentication fields and clear only transient
/// state. The library keeps its own authentication flag outside application S.
///
/// Dropping the run future or unwinding a panic closes the stream immediately;
/// it cannot execute an asynchronous close hook. No background task is spawned.
pub trait SessionHooks<S: Send + 'static>: Send + Sync {
  /// Runs before greeting OK. Success permits the session to accept commands.
  fn authenticate<'a>(&'a self, context: CommandContext<'a, S>) -> HookFuture<'a>;
  /// Applies one validated OPTION request.
  fn option<'a>(
    &'a self,
    request: OptionRequest<'a>,
    context: CommandContext<'a, S>,
  ) -> HookFuture<'a>;
  /// Clears transient application state while retaining authentication.
  fn reset<'a>(&'a self, context: CommandContext<'a, S>) -> HookFuture<'a>;
  /// Runs after the stream closes, bounded by the shutdown timeout.
  ///
  /// An earlier session error takes precedence if this hook also fails.
  fn closed<'a>(&'a self, state: &'a mut S, reason: SessionEnd) -> HookFuture<'a>;
}

/// Accepts connections already admitted by the transport; rejects every OPTION.
///
/// This policy does not authenticate TCP peers or fabricate OS identities.
/// Install application authentication when the listener policy is insufficient.
#[derive(Debug, Default)]
pub struct DefaultHooks {
  _private: (),
}

impl<S: Send + 'static> SessionHooks<S> for DefaultHooks {
  fn authenticate<'a>(&'a self, _: CommandContext<'a, S>) -> HookFuture<'a> {
    return Box::pin(async {
      return Ok(());
    });
  }

  fn option<'a>(&'a self, _: OptionRequest<'a>, _: CommandContext<'a, S>) -> HookFuture<'a> {
    return Box::pin(async {
      return Err(HandlerError::Remote {
        code: UNKNOWN_OPTION,
        message: "unknown option".into(),
      });
    });
  }

  fn reset<'a>(&'a self, _: CommandContext<'a, S>) -> HookFuture<'a> {
    return Box::pin(async {
      return Ok(());
    });
  }

  fn closed<'a>(&'a self, _: &'a mut S, _: SessionEnd) -> HookFuture<'a> {
    return Box::pin(async {
      return Ok(());
    });
  }
}
