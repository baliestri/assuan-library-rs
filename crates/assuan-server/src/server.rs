use std::{fmt, future::Future, sync::Arc};

use assuan_transport::Acceptor;
use tokio::{
  sync::Semaphore,
  task::{JoinError, JoinSet},
  time::timeout_at,
};

use crate::{
  DefaultHooks, Handler, Registry, RegistryError, ServerError, ServerOptions, Session,
  SessionHooks, options::deadline, session::end_reason,
};

/// A bounded asynchronous server over a standard or application-defined
/// acceptor.
///
/// Each accepted connection gets fresh state from the factory. Handler
/// instances and hooks are shared, while S only needs Send. No task is created
/// until a session permit and an accepted stream are available.
pub struct Server<S: Send + 'static = ()> {
  factory: Arc<dyn Fn() -> S + Send + Sync>,
  registry: Registry<S>,
  hooks: Arc<dyn SessionHooks<S>>,
  options: ServerOptions,
}

impl<S: Send + 'static> Server<S> {
  /// Configures state creation and limits. Options are checked before serving.
  #[must_use]
  pub fn new(factory: impl Fn() -> S + Send + Sync + 'static, options: ServerOptions) -> Self {
    return Self {
      factory: Arc::new(factory),
      registry: Registry::new(),
      hooks: Arc::new(DefaultHooks::default()),
      options,
    };
  }

  /// Registers a shared handler before serving.
  ///
  /// # Errors
  /// Rejects duplicate, reserved or invalid names and invalid descriptions.
  pub fn register(&mut self, handler: impl Handler<S> + 'static) -> Result<(), RegistryError> {
    return self.registry.register(handler);
  }

  /// Replaces the default policy before any sessions are accepted.
  pub fn set_hooks(&mut self, hooks: impl SessionHooks<S> + 'static) {
    self.hooks = Arc::new(hooks);
  }

  /// Accepts concurrent sessions until shutdown resolves or acceptance fails.
  ///
  /// A permit is acquired before polling accept and lives through the close
  /// hook. At capacity the acceptor is not polled; any OS or custom backlog
  /// belongs to that transport. Completed tasks are reaped throughout serving.
  ///
  /// Shutdown drops the pending accept future and the owned acceptor, then
  /// grants sessions one shared shutdown timeout. Remaining tasks are aborted
  /// and joined before return. Active sessions may finish commands or send BYE
  /// during that grace period. Aborted sessions cannot await close hooks.
  ///
  /// Session errors and task panics are isolated and logged by category only.
  /// Dropping this future aborts owned tasks through `JoinSet` but cannot await
  /// their cleanup. Cancellation must be supported by a custom acceptor.
  /// Unix socket path cleanup remains the listener owner's explicit policy;
  /// dropping the listener does not unlink its pathname.
  ///
  /// # Errors
  /// Returns invalid options or the first accept error, after draining
  /// sessions.
  ///
  /// # Panics
  /// Requires a Tokio runtime with time enabled. Acceptors and the shutdown
  /// future may propagate application panics. Factories run inside session
  /// tasks. Like all cooperative async deadlines, shutdown cannot preempt
  /// blocking code that never yields.
  pub async fn serve<A: Acceptor + 'static>(
    self,
    mut acceptor: A,
    shutdown: impl Future<Output = ()> + Send,
  ) -> Result<(), ServerError> {
    self.options.validate()?;
    let semaphore = Arc::new(Semaphore::new(self.options.max_sessions));
    let registry = Arc::new(self.registry);
    let mut tasks = JoinSet::new();
    tokio::pin!(shutdown);
    let result = 'accepting: loop {
      let permit = loop {
        tokio::select! {
          biased;
          () = &mut shutdown => break 'accepting Ok(()),
          Some(joined) = tasks.join_next(), if !tasks.is_empty() => record(joined),
          permit = Arc::clone(&semaphore).acquire_owned() => {
            match permit {
              Ok(permit) => break permit,
              Err(_) => break 'accepting Err(ServerError::InvalidOptions),
            }
          }
        }
      };
      // Keep this future alive while reaping tasks. Custom acceptors are not
      // required to restart acceptance whenever an unrelated session finishes.
      let mut incoming = acceptor.accept();
      let accepted = loop {
        tokio::select! {
          biased;
          () = &mut shutdown => break 'accepting Ok(()),
          Some(joined) = tasks.join_next(), if !tasks.is_empty() => record(joined),
          result = &mut incoming => {
            match result {
              Ok(accepted) => break accepted,
              Err(error) => break 'accepting Err(ServerError::Transport(error)),
            }
          }
        }
      };
      drop(incoming);
      let registry = Arc::clone(&registry);
      let hooks = Arc::clone(&self.hooks);
      let factory = Arc::clone(&self.factory);
      let options = self.options.clone();
      tasks.spawn(async move {
        let _permit = permit;
        let state = factory();
        return Session::new(accepted, state, registry, hooks, options).run().await;
      });
    };
    drop(acceptor);
    let grace = deadline(self.options.shutdown_timeout)?;
    if timeout_at(grace, drain(&mut tasks)).await.is_err() {
      tasks.abort_all();
      drain(&mut tasks).await;
    }
    return result;
  }
}

async fn drain(tasks: &mut JoinSet<Result<(), ServerError>>) {
  while let Some(joined) = tasks.join_next().await {
    record(joined);
  }
  return;
}

fn record(joined: Result<Result<(), ServerError>, JoinError>) {
  match joined {
    Ok(Ok(())) => {}
    Ok(Err(error)) => {
      log::warn!(target: "assuan_server", "session ended: {:?}", end_reason(&error));
    }
    Err(error) => {
      let category = if error.is_cancelled() {
        "cancelled"
      } else {
        "panic"
      };
      log::warn!(target: "assuan_server", "session task ended: {category}");
    }
  }
}

impl<S: Send + 'static> fmt::Debug for Server<S> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f
      .debug_struct("Server")
      .field("registry", &self.registry)
      .field("options", &self.options)
      .finish_non_exhaustive();
  }
}
