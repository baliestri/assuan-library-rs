use std::{
  collections::{HashMap, hash_map::Entry},
  fmt,
  marker::PhantomData,
};

use assuan_protocol::{Command, MAX_LINE_BYTES};

use crate::{CommandContext, Handler, HandlerFuture, RegistryError};

/// An explicit, case-sensitive collection of handlers for session state `S`.
///
/// Registration rejects duplicates and built-in names without replacement.
/// Lookup borrows the handler and performs no allocation. Debug reports only
/// the count, never handler captures or metadata.
pub struct Registry<S: Send + 'static = ()> {
  handlers: HashMap<String, Box<dyn Handler<S>>>,
}

impl<S: Send + 'static> Default for Registry<S> {
  fn default() -> Self {
    return Self::new();
  }
}

impl<S: Send + 'static> Registry<S> {
  /// Creates an empty registry.
  #[must_use]
  pub fn new() -> Self {
    return Self {
      handlers: HashMap::new(),
    };
  }

  /// Validates metadata and registers a handler under its current name.
  ///
  /// The key is copied once and remains stable even if the handler later
  /// changes its metadata through interior mutability.
  ///
  /// # Errors
  /// Rejects invalid or reserved names, CR/LF/NUL in descriptions, and
  /// duplicate keys. A failed registration leaves every existing entry
  /// untouched.
  pub fn register<H: Handler<S> + 'static>(&mut self, handler: H) -> Result<(), RegistryError> {
    let name = handler.name();
    validate_name(name)?;
    validate_description(handler.description())?;
    let name = name.to_owned();
    match self.handlers.entry(name) {
      Entry::Occupied(entry) => return Err(RegistryError::Duplicate(entry.key().clone())),
      Entry::Vacant(entry) => {
        entry.insert(Box::new(handler));
      }
    }
    return Ok(());
  }

  /// Borrows the handler registered under exactly `name`.
  #[must_use]
  pub fn get(&self, name: &str) -> Option<&dyn Handler<S>> {
    return self.handlers.get(name).map(|handler| return handler.as_ref());
  }
}

impl<S: Send + 'static> fmt::Debug for Registry<S> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f.debug_struct("Registry").field("len", &self.handlers.len()).finish_non_exhaustive();
  }
}

fn validate_name(name: &str) -> Result<(), RegistryError> {
  Command::new(name, b"").map_err(|_| return RegistryError::InvalidName)?;
  if name.len() >= MAX_LINE_BYTES {
    return Err(RegistryError::InvalidName);
  }
  if matches!(name, "NOP" | "BYE" | "HELP" | "RESET" | "OPTION") {
    return Err(RegistryError::Reserved(name.to_owned()));
  }
  return Ok(());
}

fn validate_description(value: &str) -> Result<(), RegistryError> {
  if value.bytes().any(|byte| return matches!(byte, 0 | b'\r' | b'\n')) {
    return Err(RegistryError::InvalidDescription);
  }
  return Ok(());
}

struct ClosureHandler<S, F> {
  name: &'static str,
  description: &'static str,
  function: F,
  marker: PhantomData<fn(S)>,
}

impl<S: Send + 'static, F> Handler<S> for ClosureHandler<S, F>
where F: for<'a> Fn(Command<'a>, CommandContext<'a, S>) -> HandlerFuture<'a> + Send + Sync + 'static
{
  fn name(&self) -> &str {
    return self.name;
  }

  fn description(&self) -> &str {
    return self.description;
  }

  fn call<'a>(&'a self, command: Command<'a>, context: CommandContext<'a, S>) -> HandlerFuture<'a> {
    return (self.function)(command, context);
  }
}

/// Adapts a function or closure to a typed handler without a procedural macro.
///
/// The callback accepts any invocation lifetime and may keep arguments borrowed
/// across awaits. Its captures must be Send + Sync; session state only needs
/// Send. Each invocation returns one boxed future.
///
/// # Errors
/// Returns a registration error for invalid/reserved names or CR/LF/NUL in
/// descriptions. Duplicate detection happens when adding to a registry.
///
/// # Examples
/// ```
/// # fn main() -> Result<(), assuan_server::RegistryError> {
/// use assuan_protocol::Command;
/// use assuan_server::{CommandContext, HandlerFuture, Registry, handler};
///
/// fn echo<'a>(command: Command<'a>, mut ctx: CommandContext<'a>) -> HandlerFuture<'a> {
///   return Box::pin(async move {
///     tokio::task::yield_now().await;
///     return ctx.send_data(command.args()).await;
///   });
/// }
/// let mut registry = Registry::<()>::new();
/// registry.register(handler("ECHO", "Returns the command arguments", echo)?)?;
/// # return Ok(());
/// # }
/// ```
///
/// Borrowed command data cannot escape into session state:
///
/// ```compile_fail,E0521
/// use assuan_protocol::Command;
/// use assuan_server::{CommandContext, HandlerFuture};
/// fn retain<'a>(command: Command<'a>, mut ctx: CommandContext<'a, Option<&'static [u8]>>) -> HandlerFuture<'a> {
///   return Box::pin(async move {
///     *ctx.state_mut() = Some(command.args());
///     return Ok(());
///   });
/// }
/// ```
pub fn handler<S: Send + 'static, F>(
  name: &'static str,
  description: &'static str,
  function: F,
) -> Result<impl Handler<S>, RegistryError>
where
  F: for<'a> Fn(Command<'a>, CommandContext<'a, S>) -> HandlerFuture<'a> + Send + Sync + 'static,
{
  validate_name(name)?;
  validate_description(description)?;
  return Ok(ClosureHandler {
    name,
    description,
    function,
    marker: PhantomData,
  });
}
