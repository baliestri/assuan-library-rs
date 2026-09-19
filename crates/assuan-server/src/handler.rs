use crate::{CommandContext, HandlerError};
use assuan_protocol::Command;
use std::{future::Future, pin::Pin};

/// A handler operation borrowing its command, context, and handler.
///
/// One boxed future per invocation permits heterogeneous handlers in a registry.
/// Arguments may remain borrowed across awaits, but cannot outlive the call.
pub type HandlerFuture<'a> = Pin<Box<dyn Future<Output = Result<(), HandlerError>> + Send + 'a>>;

/// A named asynchronous command operating on session-local state.
///
/// Implementations are shared across sessions and must be thread-safe; `S` only
/// needs `Send` because each session owns its state exclusively. Metadata should
/// remain stable after registration. Names are case-sensitive.
///
/// Returning success asks the session runner to send the final OK; returning
/// an error asks it to apply its error policy. Handlers never send a final
/// response themselves.
pub trait Handler<S: Send + 'static = ()>: Send + Sync {
  /// Returns the wire command name, validated when registered.
  fn name(&self) -> &str;
  /// Returns public HELP text without CR, LF, or NUL.
  fn description(&self) -> &str;
  /// Executes using exclusive access to this session's context.
  ///
  /// The returned future may borrow all three inputs for the duration of the
  /// call. It must not retain command bytes in session state.
  fn call<'a>(&'a self, command: Command<'a>, context: CommandContext<'a, S>) -> HandlerFuture<'a>;
}
