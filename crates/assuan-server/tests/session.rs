//! Session lifecycle with an independent literal peer.
use std::sync::Arc;

use assuan_server::{CommandContext, DefaultHooks, HandlerFuture, Registry, Session, handler};
use assuan_transport::{Accepted, Stream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn echo<'a>(cmd: assuan_protocol::Command<'a>, mut ctx: CommandContext<'a>) -> HandlerFuture<'a> {
  return Box::pin(async move {
    return ctx.send_data(cmd.args()).await;
  });
}

#[tokio::test]
async fn handler_produces_exactly_one_final_response() {
  let (io, mut peer) = tokio::io::duplex(128);
  let mut registry = Registry::new();
  registry.register(handler("ECHO", "Echo", echo).unwrap()).unwrap();
  let session = Session::new(
    Accepted {
      stream: Stream::new(io),
      peer: None,
    },
    (),
    Arc::new(registry),
    Arc::new(DefaultHooks::default()),
    ServerOptions::default(),
  );
  let run = tokio::spawn(session.run());
  let mut greeting = [0; 3];
  peer.read_exact(&mut greeting).await.unwrap();
  assert_eq!(&greeting, b"OK\n");
  peer.write_all(b"ECHO payload\n").await.unwrap();
  let mut response = [0; 13];
  peer.read_exact(&mut response).await.unwrap();
  assert_eq!(&response, b"D payload\nOK\n");
  peer.shutdown().await.unwrap();
  let mut remaining = Vec::new();
  peer.read_to_end(&mut remaining).await.unwrap();
  assert!(remaining.is_empty());
  run.await.unwrap().unwrap();
}
use assuan_server::{HandlerError, ServerError, ServerOptions};
use assuan_transport::TransportError;
mod support;
use support::{defaults, eof, expect, start};

fn fail<'a>(cmd: assuan_protocol::Command<'a>, _: CommandContext<'a>) -> HandlerFuture<'a> {
  return Box::pin(async move {
    return match cmd.args() {
      b"remote" => Err(HandlerError::remote(u32::MAX, "denied").unwrap()),
      b"internal" => Err(HandlerError::Internal),
      b"invalid" => {
        Err(HandlerError::Remote {
          code: 7,
          message: "SECRET_MARKER\nOK".into(),
        })
      }
      b"transport" => {
        Err(HandlerError::Transport(TransportError::Io(std::io::Error::other("SECRET_MARKER"))))
      }
      b"panic" => panic!("test handler panic"),
      b"pending" => std::future::pending().await,
      _ => Ok(()),
    };
  });
}

fn failure_registry() -> Registry {
  let mut registry = Registry::new();
  registry.register(handler("FAIL", "Failure cases", fail).unwrap()).unwrap();
  return registry;
}

#[tokio::test]
async fn remote_and_internal_errors_each_send_one_final_and_preserve_reuse() {
  let (mut peer, run) = start((), failure_registry(), defaults(), ServerOptions::default());
  expect(&mut peer, b"OK\n").await;
  for (cmd, expected) in [
    (b"FAIL remote\n".as_slice(), b"ERR 4294967295 denied\n".as_slice()),
    (b"FAIL internal\n", b"ERR 63 internal error\n"),
    (b"FAIL success\n", b"OK\n"),
  ] {
    peer.write_all(cmd).await.unwrap();
    expect(&mut peer, expected).await;
  }
  peer.shutdown().await.unwrap();
  eof(&mut peer).await;
  run.await.unwrap().unwrap();
}

#[tokio::test]
async fn invalid_remote_fields_and_transport_failures_close_without_a_final() {
  for arg in ["invalid", "transport"] {
    let (mut peer, run) = start((), failure_registry(), defaults(), ServerOptions::default());
    expect(&mut peer, b"OK\n").await;
    peer.write_all(format!("FAIL {arg}\n").as_bytes()).await.unwrap();
    eof(&mut peer).await;
    let err = run.await.unwrap().unwrap_err();
    assert!(!format!("{err:?} {err}").contains("SECRET_MARKER"));
  }
}

#[tokio::test]
async fn idle_eof_is_clean_but_partial_lines_and_out_of_phase_input_are_fatal() {
  for input in
    [b"unfinished".as_slice(), b"D data\n", b"END\n", b"CAN\n", b"CAN invalid\n", b"#bad\0\n"]
  {
    let (mut peer, run) = start((), Registry::new(), defaults(), ServerOptions::default());
    expect(&mut peer, b"OK\n").await;
    peer.write_all(input).await.unwrap();
    peer.shutdown().await.unwrap();
    eof(&mut peer).await;
    assert!(run.await.unwrap().is_err());
  }
}

#[tokio::test(start_paused = true)]
async fn total_deadline_includes_handler_work_without_io() {
  let options = ServerOptions {
    command_timeout: std::time::Duration::from_secs(2),
    ..Default::default()
  };
  let (mut peer, run) = start((), failure_registry(), defaults(), options);
  expect(&mut peer, b"OK\n").await;
  peer.write_all(b"FAIL pending\n").await.unwrap();
  eof(&mut peer).await;
  assert!(matches!(run.await.unwrap(), Err(ServerError::Transport(TransportError::Timeout))));
}

#[tokio::test]
async fn panic_or_aborted_session_drops_the_stream() {
  let (mut peer, run) = start((), failure_registry(), defaults(), ServerOptions::default());
  expect(&mut peer, b"OK\n").await;
  peer.write_all(b"FAIL panic\n").await.unwrap();
  assert!(run.await.unwrap_err().is_panic());
  eof(&mut peer).await;
  let (mut peer, run) = start((), Registry::new(), defaults(), ServerOptions::default());
  expect(&mut peer, b"OK\n").await;
  run.abort();
  assert!(run.await.unwrap_err().is_cancelled());
  eof(&mut peer).await;
}

#[tokio::test]
async fn invalid_options_close_before_greeting() {
  let options = ServerOptions {
    max_inquiry_bytes: 0,
    ..Default::default()
  };
  let (mut peer, run) = start((), Registry::new(), defaults(), options);
  eof(&mut peer).await;
  assert!(matches!(run.await.unwrap(), Err(ServerError::InvalidOptions)));
}
