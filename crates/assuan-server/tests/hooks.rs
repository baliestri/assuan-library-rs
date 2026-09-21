//! Authentication, typed options, reset, and close-hook behavior.
use std::{
  cell::Cell,
  sync::{Arc, Mutex},
  time::Duration,
};

use assuan_protocol::{PayloadRef, Sensitivity};
use assuan_server::{
  CommandContext, HandlerError, HookFuture, InquiryOutcome, OptionRequest, Registry, ServerError,
  ServerOptions, SessionEnd, SessionHooks,
};
use assuan_transport::TransportError;
use tokio::io::AsyncWriteExt;
mod support;
use support::{defaults, eof, expect, start};

#[derive(Default)]
struct State {
  authenticated: Cell<bool>,
  transient: usize,
}

struct Hooks {
  observed: Arc<Mutex<Vec<(bool, usize, SessionEnd)>>>,
  reject: bool,
  fail_close: bool,
}

impl SessionHooks<State> for Hooks {
  fn authenticate<'a>(&'a self, mut ctx: CommandContext<'a, State>) -> HookFuture<'a> {
    return Box::pin(async move {
      assert!(!ctx.is_authenticated());
      assert!(ctx.peer().is_none());
      assert!(!format!("{ctx:?}").contains("SECRET_MARKER"));
      let mut inquiry = ctx.inquire("AUTH", b"", Sensitivity::Secret).await?;
      let Some(PayloadRef::Secret(answer)) = inquiry.next().await? else {
        return Err(HandlerError::Internal);
      };
      assert_eq!(answer.expose(), b"SECRET_MARKER");
      assert!(inquiry.next().await?.is_none());
      assert_eq!(inquiry.finish()?, InquiryOutcome::Completed);
      if self.reject {
        return Err(HandlerError::remote(99, "rejected").unwrap());
      }
      ctx.state_mut().authenticated.set(true);
      return Ok(());
    });
  }

  fn option<'a>(
    &'a self,
    request: OptionRequest<'a>,
    mut ctx: CommandContext<'a, State>,
  ) -> HookFuture<'a> {
    return Box::pin(async move {
      assert!(ctx.is_authenticated());
      assert!(ctx.state().authenticated.get());
      assert_eq!(request.name(), b"count");
      assert_eq!(request.value(), Some(b"SECRET_MARKER".as_slice()));
      assert!(!format!("{request:?}").contains("SECRET_MARKER"));
      ctx.state_mut().transient += 1;
      return Ok(());
    });
  }

  fn reset<'a>(&'a self, mut ctx: CommandContext<'a, State>) -> HookFuture<'a> {
    return Box::pin(async move {
      assert!(ctx.is_authenticated());
      assert!(ctx.state().authenticated.get());
      assert_eq!(ctx.state().transient, 1);
      ctx.state_mut().transient = 0;
      return Ok(());
    });
  }

  fn closed<'a>(&'a self, state: &'a mut State, reason: SessionEnd) -> HookFuture<'a> {
    return Box::pin(async move {
      self.observed.lock().unwrap().push((state.authenticated.get(), state.transient, reason));
      if self.fail_close {
        return Err(HandlerError::remote(7, "SECRET_MARKER").unwrap());
      }
      return Ok(());
    });
  }
}

#[tokio::test]
async fn authentication_inquiry_precedes_greeting_and_reset_preserves_authentication() {
  let observed = Arc::new(Mutex::new(Vec::new()));
  let hooks = Arc::new(Hooks {
    observed: observed.clone(),
    reject: false,
    fail_close: false,
  });
  let (mut peer, run) = start(State::default(), Registry::new(), hooks, ServerOptions::default());
  expect(&mut peer, b"INQUIRE AUTH\n").await;
  peer.write_all(b"D SECRET_MARKER\nEND\n").await.unwrap();
  expect(&mut peer, b"OK\n").await;
  peer.write_all(b"OPTION --count = SECRET_MARKER\n").await.unwrap();
  expect(&mut peer, b"OK\n").await;
  peer.write_all(b"RESET\n").await.unwrap();
  expect(&mut peer, b"OK\n").await;
  peer.shutdown().await.unwrap();
  eof(&mut peer).await;
  run.await.unwrap().unwrap();
  assert_eq!(*observed.lock().unwrap(), [(true, 0, SessionEnd::Clean)]);
}

struct Capture(Mutex<Vec<String>>);
static LOG: Capture = Capture(Mutex::new(Vec::new()));
impl log::Log for Capture {
  fn enabled(&self, _: &log::Metadata<'_>) -> bool {
    return true;
  }

  fn log(&self, record: &log::Record<'_>) {
    self.0.lock().unwrap().push(format!("{}", record.args()));
  }

  fn flush(&self) {}
}

#[tokio::test]
async fn rejected_authentication_preserves_original_error_when_closed_also_fails() {
  log::set_logger(&LOG).unwrap();
  log::set_max_level(log::LevelFilter::Warn);
  let observed = Arc::new(Mutex::new(Vec::new()));
  let hooks = Arc::new(Hooks {
    observed: observed.clone(),
    reject: true,
    fail_close: true,
  });
  let (mut peer, run) = start(State::default(), Registry::new(), hooks, ServerOptions::default());
  expect(&mut peer, b"INQUIRE AUTH\n").await;
  peer.write_all(b"D SECRET_MARKER\nEND\n").await.unwrap();
  expect(&mut peer, b"ERR 99 rejected\n").await;
  eof(&mut peer).await;
  assert!(matches!(
    run.await.unwrap(),
    Err(ServerError::Handler(HandlerError::Remote {
      code: 99,
      ..
    }))
  ));
  assert_eq!(*observed.lock().unwrap(), [(false, 0, SessionEnd::Handler)]);
  let logs = LOG.0.lock().unwrap();
  assert!(logs.iter().any(|line| return line == "close hook failed: Handler"));
  assert!(logs.iter().all(|line| return !line.contains("SECRET_MARKER")));
}

#[tokio::test]
async fn default_hooks_reject_unknown_options_and_reset_succeeds() {
  let (mut peer, run) = start((), Registry::new(), defaults(), ServerOptions::default());
  expect(&mut peer, b"OK\n").await;
  peer.write_all(b"OPTION unknown=value\n").await.unwrap();
  expect(&mut peer, b"ERR 174 unknown option\n").await;
  peer.write_all(b"OPTION =value\n").await.unwrap();
  expect(&mut peer, b"ERR 55 invalid option\n").await;
  peer.write_all(b"RESET\n").await.unwrap();
  expect(&mut peer, b"OK\n").await;
  peer.shutdown().await.unwrap();
  eof(&mut peer).await;
  run.await.unwrap().unwrap();
}

#[test]
fn option_parsing_preserves_borrowed_binary_values_and_distinguishes_empty() {
  for input in [b" --name = value ".as_slice(), b"name value", b"name=value", b"--name\tvalue"] {
    let option = OptionRequest::parse(input).unwrap();
    assert_eq!(option.name(), b"name");
    assert_eq!(option.value(), Some(b"value".as_slice()));
  }
  assert_eq!(OptionRequest::parse(b"name=").unwrap().value(), Some(b"".as_slice()));
  assert_eq!(OptionRequest::parse(b"name").unwrap().value(), None);
  assert_eq!(OptionRequest::parse(b"name=\xff%00").unwrap().value(), Some(b"\xff%00".as_slice()));
  for invalid in [b"".as_slice(), b"=x", b"--", b"name=\0", b"na%me=value"] {
    assert!(OptionRequest::parse(invalid).is_err());
  }
}

struct SlowHooks {
  close: bool,
}
impl SessionHooks<()> for SlowHooks {
  fn authenticate<'a>(&'a self, _: CommandContext<'a>) -> HookFuture<'a> {
    return Box::pin(async move {
      if !self.close {
        std::future::pending::<()>().await;
      }
      return Ok(());
    });
  }

  fn option<'a>(&'a self, _: OptionRequest<'a>, _: CommandContext<'a>) -> HookFuture<'a> {
    return Box::pin(async {
      return Ok(());
    });
  }

  fn reset<'a>(&'a self, _: CommandContext<'a>) -> HookFuture<'a> {
    return Box::pin(async {
      return Ok(());
    });
  }

  fn closed<'a>(&'a self, (): &'a mut (), reason: SessionEnd) -> HookFuture<'a> {
    return Box::pin(async move {
      if self.close {
        std::future::pending::<()>().await;
      }
      assert_eq!(reason, SessionEnd::Timeout);
      return Ok(());
    });
  }
}

#[tokio::test(start_paused = true)]
async fn authentication_and_close_hooks_have_bounded_total_deadlines() {
  for close in [false, true] {
    let options = ServerOptions {
      greeting_timeout: Duration::from_secs(2),
      shutdown_timeout: Duration::from_secs(2),
      ..Default::default()
    };
    let (mut peer, run) = start(
      (),
      Registry::new(),
      Arc::new(SlowHooks {
        close,
      }),
      options,
    );
    if close {
      expect(&mut peer, b"OK\n").await;
      peer.shutdown().await.unwrap();
    }
    eof(&mut peer).await;
    assert!(matches!(run.await.unwrap(), Err(ServerError::Transport(TransportError::Timeout))));
  }
}
