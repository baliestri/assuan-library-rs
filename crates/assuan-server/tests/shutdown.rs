//! Custom acceptors, bounded concurrency and deterministic shutdown.
use std::{
  sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
  },
  time::Duration,
};

use assuan_protocol::Command;
use assuan_server::{CommandContext, HandlerFuture, Server, ServerOptions, handler};
use assuan_transport::Acceptor;
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  sync::oneshot,
  time::Instant,
};
#[path = "support/acceptor.rs"]
mod custom;
use custom::{acceptor, connect};

async fn expect(peer: &mut tokio::io::DuplexStream, bytes: &[u8]) {
  let mut got = vec![0; bytes.len()];
  peer.read_exact(&mut got).await.unwrap();
  assert_eq!(got, bytes);
}
async fn eof(peer: &mut tokio::io::DuplexStream) {
  assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
}
fn increment<'a>(_: Command<'a>, mut ctx: CommandContext<'a, usize>) -> HandlerFuture<'a> {
  return Box::pin(async move {
    *ctx.state_mut() += 1;
    return ctx.send_data(ctx.state().to_string().as_bytes()).await;
  });
}

#[tokio::test]
async fn stalled_connection_does_not_block_another_and_state_is_independent() {
  let (sender, acceptor, stats) = acceptor();
  assert!(acceptor.endpoint().is_none());
  let mut server =
    Server::new(|| return 0_usize, ServerOptions::default().with_max_sessions(2).unwrap());
  server.register(handler("INC", "Increment", increment).unwrap()).unwrap();
  let (stop, stopped) = oneshot::channel();
  let run = tokio::spawn(server.serve(acceptor, async {
    let _ = stopped.await;
  }));
  let mut first = connect(&sender).await;
  expect(&mut first, b"OK\n").await;
  let mut second = connect(&sender).await;
  expect(&mut second, b"OK\n").await;
  second.write_all(b"INC\nINC\n").await.unwrap();
  expect(&mut second, b"D 1\nOK\nD 2\nOK\n").await;
  first.write_all(b"INC\n").await.unwrap();
  expect(&mut first, b"D 1\nOK\n").await;
  assert_eq!(stats.accepted.load(Ordering::SeqCst), 2);
  first.write_all(b"BYE\n").await.unwrap();
  second.write_all(b"BYE\n").await.unwrap();
  expect(&mut first, b"OK\n").await;
  expect(&mut second, b"OK\n").await;
  stop.send(()).unwrap();
  run.await.unwrap().unwrap();
  eof(&mut first).await;
  eof(&mut second).await;
}

#[tokio::test]
async fn one_session_limit_stops_acceptance_until_the_permit_is_released() {
  let (sender, acceptor, stats) = acceptor();
  let server = Server::new(|| (), ServerOptions::default().with_max_sessions(1).unwrap());
  let (stop, stopped) = oneshot::channel();
  let run = tokio::spawn(server.serve(acceptor, async {
    let _ = stopped.await;
  }));
  let mut first = connect(&sender).await;
  expect(&mut first, b"OK\n").await;
  let mut second = connect(&sender).await;
  tokio::task::yield_now().await;
  assert_eq!(stats.accepted.load(Ordering::SeqCst), 1);
  assert_eq!(stats.polls.load(Ordering::SeqCst), 1);
  first.write_all(b"BYE\n").await.unwrap();
  expect(&mut first, b"OK\n").await;
  expect(&mut second, b"OK\n").await;
  assert_eq!(stats.accepted.load(Ordering::SeqCst), 2);
  second.write_all(b"BYE\n").await.unwrap();
  expect(&mut second, b"OK\n").await;
  stop.send(()).unwrap();
  run.await.unwrap().unwrap();
}

struct Tracked(Arc<AtomicUsize>);
fn hang<'a>(_: Command<'a>, mut ctx: CommandContext<'a, Tracked>) -> HandlerFuture<'a> {
  return Box::pin(async move {
    ctx.send_status("STARTED", b"").await?;
    return std::future::pending().await;
  });
}
impl Drop for Tracked {
  fn drop(&mut self) {
    self.0.fetch_add(1, Ordering::SeqCst);
  }
}

#[tokio::test(start_paused = true)]
async fn shutdown_stops_accepting_then_aborts_and_joins_after_one_shared_grace_period() {
  let (sender, acceptor, stats) = acceptor();
  let dropped = Arc::new(AtomicUsize::new(0));
  let factory_dropped = dropped.clone();
  let options = ServerOptions::default()
    .with_max_sessions(1)
    .unwrap()
    .with_shutdown_timeout(Duration::from_secs(2))
    .unwrap();
  let mut server = Server::new(move || return Tracked(factory_dropped.clone()), options);
  server.register(handler("HANG", "Wait indefinitely", hang).unwrap()).unwrap();
  let (stop, stopped) = oneshot::channel();
  let run = tokio::spawn(server.serve(acceptor, async {
    let _ = stopped.await;
  }));
  let mut first = connect(&sender).await;
  expect(&mut first, b"OK\n").await;
  let mut queued = connect(&sender).await;
  first.write_all(b"HANG\n").await.unwrap();
  expect(&mut first, b"S STARTED\n").await;
  let before = Instant::now();
  stop.send(()).unwrap();
  tokio::task::yield_now().await;
  assert!(stats.dropped.load(Ordering::SeqCst));
  assert_eq!(stats.accepted.load(Ordering::SeqCst), 1);
  run.await.unwrap().unwrap();
  assert_eq!(Instant::now() - before, Duration::from_secs(2));
  assert_eq!(dropped.load(Ordering::SeqCst), 1);
  eof(&mut first).await;
  eof(&mut queued).await;
}

#[tokio::test(start_paused = true)]
async fn sessions_can_complete_during_grace_without_waiting_for_its_deadline() {
  let (sender, acceptor, _) = acceptor();
  let server = Server::new(|| (), ServerOptions::default());
  let (stop, stopped) = oneshot::channel();
  let run = tokio::spawn(server.serve(acceptor, async {
    let _ = stopped.await;
  }));
  let mut peer = connect(&sender).await;
  expect(&mut peer, b"OK\n").await;
  let before = Instant::now();
  stop.send(()).unwrap();
  peer.write_all(b"NOP\nBYE\n").await.unwrap();
  expect(&mut peer, b"OK\nOK\n").await;
  run.await.unwrap().unwrap();
  assert_eq!(Instant::now(), before);
  eof(&mut peer).await;
}

#[tokio::test(start_paused = true)]
async fn idle_deadline_releases_capacity_without_a_client_command() {
  let (sender, acceptor, stats) = acceptor();
  let options = ServerOptions::default()
    .with_max_sessions(1)
    .unwrap()
    .with_idle_timeout(Duration::from_secs(2))
    .unwrap();
  let server = Server::new(|| (), options);
  let (stop, stopped) = oneshot::channel();
  let run = tokio::spawn(server.serve(acceptor, async {
    let _ = stopped.await;
  }));
  let mut first = connect(&sender).await;
  expect(&mut first, b"OK\n").await;
  let mut second = connect(&sender).await;
  eof(&mut first).await;
  expect(&mut second, b"OK\n").await;
  assert_eq!(stats.accepted.load(Ordering::SeqCst), 2);
  second.write_all(b"BYE\n").await.unwrap();
  expect(&mut second, b"OK\n").await;
  stop.send(()).unwrap();
  run.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn accept_error_drains_sessions_and_preserves_the_transport_error() {
  let (sender, acceptor, stats) = acceptor();
  let options = ServerOptions::default().with_shutdown_timeout(Duration::from_secs(2)).unwrap();
  let server = Server::new(|| (), options);
  let run = tokio::spawn(server.serve(acceptor, std::future::pending()));
  let mut peer = connect(&sender).await;
  expect(&mut peer, b"OK\n").await;
  drop(sender);
  assert!(matches!(
    run.await.unwrap(),
    Err(assuan_server::ServerError::Transport(assuan_transport::TransportError::Closed))
  ));
  assert!(stats.dropped.load(Ordering::SeqCst));
  eof(&mut peer).await;
}

#[test]
fn option_builders_reject_invalid_resource_limits() {
  assert!(ServerOptions::default().with_max_sessions(0).is_err());
  assert!(ServerOptions::default().with_max_sessions(usize::MAX).is_err());
  assert!(ServerOptions::default().with_max_inquiry_bytes(0).is_err());
  assert!(ServerOptions::default().with_greeting_timeout(Duration::ZERO).is_err());
  assert!(ServerOptions::default().with_command_timeout(Duration::MAX).is_err());
  assert!(ServerOptions::default().with_inquiry_timeout(Duration::ZERO).is_err());
  assert!(ServerOptions::default().with_idle_timeout(Duration::ZERO).is_err());
  assert!(ServerOptions::default().with_shutdown_timeout(Duration::ZERO).is_err());
}
#[tokio::test]
async fn client_from_stream_works_with_an_acceptor_without_an_endpoint() {
  use assuan_client::{Client, ClientOptions, CollectLimits};
  let (sender, acceptor, stats) = acceptor();
  assert!(acceptor.endpoint().is_none());
  let mut server = Server::new(|| return 0_usize, ServerOptions::default());
  server.register(handler("INC", "Increment", increment).unwrap()).unwrap();
  let (stop, stopped) = oneshot::channel();
  let run = tokio::spawn(server.serve(acceptor, async {
    let _ = stopped.await;
  }));
  let peer = connect(&sender).await;
  let mut client =
    Client::from_stream(assuan_transport::Stream::new(peer), ClientOptions::default())
      .await
      .unwrap();
  let response =
    client.collect(Command::new("INC", b"").unwrap(), CollectLimits::default()).await.unwrap();
  assert_eq!(response.data(), b"1");
  client.collect(Command::new("BYE", b"").unwrap(), CollectLimits::default()).await.unwrap();
  drop(client);
  stop.send(()).unwrap();
  run.await.unwrap().unwrap();
  assert_eq!(stats.accepted.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn tcp_listener_serves_the_real_client() {
  use assuan_client::{Client, ClientOptions, CollectLimits};
  use assuan_transport::{Endpoint, ListenOptions, Listener};
  let listener =
    Listener::bind(&Endpoint::Tcp("127.0.0.1:0".parse().unwrap()), &ListenOptions::default())
      .await
      .unwrap();
  let endpoint = listener.endpoint().clone();
  let mut server = Server::new(|| return 0_usize, ServerOptions::default());
  server.register(handler("INC", "Increment", increment).unwrap()).unwrap();
  let (stop, stopped) = oneshot::channel();
  let run = tokio::spawn(server.serve(listener, async {
    let _ = stopped.await;
  }));
  let mut client = Client::connect(&endpoint, ClientOptions::default()).await.unwrap();
  assert_eq!(
    client
      .collect(Command::new("INC", b"").unwrap(), CollectLimits::default())
      .await
      .unwrap()
      .data(),
    b"1"
  );
  client.collect(Command::new("BYE", b"").unwrap(), CollectLimits::default()).await.unwrap();
  stop.send(()).unwrap();
  run.await.unwrap().unwrap();
}

#[tokio::test]
async fn factory_panic_releases_capacity_and_does_not_stop_serving() {
  let (sender, acceptor, stats) = acceptor();
  let calls = Arc::new(AtomicUsize::new(0));
  let counter = calls.clone();
  let server = Server::new(
    move || {
      assert!(counter.fetch_add(1, Ordering::SeqCst) != 0, "test factory panic");
    },
    ServerOptions::default().with_max_sessions(1).unwrap(),
  );
  let (stop, stopped) = oneshot::channel();
  let run = tokio::spawn(server.serve(acceptor, async {
    let _ = stopped.await;
  }));
  let mut first = connect(&sender).await;
  eof(&mut first).await;
  let mut second = connect(&sender).await;
  expect(&mut second, b"OK\n").await;
  second.write_all(b"BYE\n").await.unwrap();
  expect(&mut second, b"OK\n").await;
  stop.send(()).unwrap();
  run.await.unwrap().unwrap();
  assert_eq!(calls.load(Ordering::SeqCst), 2);
  assert_eq!(stats.accepted.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn immediate_shutdown_does_not_poll_accept_or_create_state() {
  let (sender, acceptor, stats) = acceptor();
  let mut queued = connect(&sender).await;
  let server = Server::new(|| panic!("factory must not run"), ServerOptions::default());
  server.serve(acceptor, std::future::ready(())).await.unwrap();
  assert_eq!(stats.polls.load(Ordering::SeqCst), 0);
  eof(&mut queued).await;
}
struct SlowClose;
impl assuan_server::SessionHooks<()> for SlowClose {
  fn authenticate<'a>(&'a self, mut ctx: CommandContext<'a>) -> assuan_server::HookFuture<'a> {
    return Box::pin(async move {
      return ctx.send_status("AUTH", b"checked").await;
    });
  }

  fn option<'a>(
    &'a self,
    _: assuan_server::OptionRequest<'a>,
    _: CommandContext<'a>,
  ) -> assuan_server::HookFuture<'a> {
    return Box::pin(async {
      return Ok(());
    });
  }

  fn reset<'a>(&'a self, _: CommandContext<'a>) -> assuan_server::HookFuture<'a> {
    return Box::pin(async {
      return Ok(());
    });
  }

  fn closed<'a>(
    &'a self,
    (): &'a mut (),
    _: assuan_server::SessionEnd,
  ) -> assuan_server::HookFuture<'a> {
    return Box::pin(async {
      tokio::time::sleep(Duration::from_secs(5)).await;
      return Ok(());
    });
  }
}

#[tokio::test(start_paused = true)]
async fn session_permit_is_retained_until_the_close_hook_completes() {
  let (sender, acceptor, stats) = acceptor();
  let mut server = Server::new(|| (), ServerOptions::default().with_max_sessions(1).unwrap());
  server.set_hooks(SlowClose);
  let (stop, stopped) = oneshot::channel();
  let run = tokio::spawn(server.serve(acceptor, async {
    let _ = stopped.await;
  }));
  let mut first = connect(&sender).await;
  expect(&mut first, b"S AUTH checked\nOK\n").await;
  let mut second = connect(&sender).await;
  let before = Instant::now();
  first.write_all(b"BYE\n").await.unwrap();
  expect(&mut first, b"OK\n").await;
  eof(&mut first).await;
  assert_eq!(stats.accepted.load(Ordering::SeqCst), 1);
  expect(&mut second, b"S AUTH checked\nOK\n").await;
  assert_eq!(Instant::now() - before, Duration::from_secs(5));
  second.write_all(b"BYE\n").await.unwrap();
  expect(&mut second, b"OK\n").await;
  stop.send(()).unwrap();
  run.await.unwrap().unwrap();
}

#[tokio::test]
async fn directly_invalid_options_are_rejected_before_accepting() {
  let (sender, acceptor, stats) = acceptor();
  let mut peer = connect(&sender).await;
  let options = ServerOptions {
    max_sessions: usize::MAX,
    ..Default::default()
  };
  let server = Server::new(|| (), options);
  assert!(matches!(
    server.serve(acceptor, std::future::pending()).await,
    Err(assuan_server::ServerError::InvalidOptions)
  ));
  assert_eq!(stats.polls.load(Ordering::SeqCst), 0);
  eof(&mut peer).await;
}
