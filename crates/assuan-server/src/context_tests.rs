use std::{
  cell::Cell,
  future::Future,
  task::{Context, Poll, Waker},
  time::Duration,
};

use assuan_protocol::{Command, RequestKind, ServerState};
use assuan_transport::Stream;
use tokio::io::AsyncReadExt;

use super::*;
use crate::{HandlerFuture, Registry, handler};

fn machine() -> ServerMachine {
  let mut machine = ServerMachine::new();
  machine.send_response(LineKind::Ok).unwrap();
  machine.receive_request(RequestKind::Command).unwrap();
  return machine;
}

fn echo<'a>(
  command: Command<'a>,
  mut context: CommandContext<'a, Cell<usize>>,
) -> HandlerFuture<'a> {
  return Box::pin(async move {
    tokio::task::yield_now().await;
    context.state_mut().set(command.args().len());
    return context.send_data(command.args()).await;
  });
}

#[tokio::test]
async fn registered_handler_borrows_arguments_across_await_and_mutates_non_sync_state() {
  let (local, mut peer) = tokio::io::duplex(8);
  let mut channel = Channel::new(Stream::new(local));
  let mut machine = machine();
  let mut state = Cell::new(0);
  let mut registry = Registry::new();
  registry.register(handler("ECHO", "Echo arguments", echo).unwrap()).unwrap();
  let args = vec![b'a', b'%', 0xFF];
  let command = Command::new("ECHO", &args).unwrap();
  let context = CommandContext {
    channel: &mut channel,
    machine: &mut machine,
    state: &mut state,
    deadline: Instant::now() + Duration::from_secs(1),
  };
  let mut wire = [0; 8];
  let (sent, received) =
    tokio::join!(registry.get("ECHO").unwrap().call(command, context), peer.read_exact(&mut wire));
  sent.unwrap();
  received.unwrap();
  assert_eq!(&wire, b"D a%25\xff\n");
  assert_eq!(state.get(), 3);
  assert_eq!(machine.state(), ServerState::Handler);
}

#[tokio::test]
async fn empty_data_and_large_escaped_payload_have_exact_wire_representation() {
  let (local, mut peer) = tokio::io::duplex(17);
  let mut channel = Channel::new(Stream::new(local));
  let mut machine = machine();
  let mut state = ();
  let mut context = CommandContext {
    channel: &mut channel,
    machine: &mut machine,
    state: &mut state,
    deadline: Instant::now() + Duration::from_secs(1),
  };
  let payload = vec![b'%'; 700];
  // 997 payload bytes per line allow 332 complete three-byte escapes.
  let expected =
    format!("D \nD {}\nD {}\nD {}\n", "%25".repeat(332), "%25".repeat(332), "%25".repeat(36));
  let mut wire = vec![0; expected.len()];
  let send = async {
    context.send_data(b"").await.unwrap();
    context.send_data(&payload).await.unwrap();
  };
  let ((), received) = tokio::join!(send, peer.read_exact(&mut wire));
  received.unwrap();
  assert_eq!(wire, expected.as_bytes());
  assert_eq!(machine.state(), ServerState::Handler);
}

#[tokio::test]
async fn cancellation_after_partial_write_closes_channel_and_invalidates_state() {
  let (local, mut peer) = tokio::io::duplex(1);
  let mut channel = Channel::new(Stream::new(local));
  let mut machine = machine();
  let mut state = ();
  {
    let mut context = CommandContext {
      channel: &mut channel,
      machine: &mut machine,
      state: &mut state,
      deadline: Instant::now() + Duration::from_secs(1),
    };
    let mut send = Box::pin(context.send_data(b"secret"));
    assert!(matches!(send.as_mut().poll(&mut Context::from_waker(Waker::noop())), Poll::Pending));
    drop(send);
    assert!(matches!(context.send_data(b"retry").await, Err(HandlerError::State(_))));
  }
  assert_eq!(machine.state(), ServerState::Invalid);
  let mut prefix = Vec::new();
  peer.read_to_end(&mut prefix).await.unwrap();
  assert_eq!(prefix, b"D");
}

#[tokio::test(start_paused = true)]
async fn expired_total_deadline_closes_channel() {
  let (local, mut peer) = tokio::io::duplex(1);
  let mut channel = Channel::new(Stream::new(local));
  let mut machine = machine();
  let mut state = ();
  let mut context = CommandContext {
    channel: &mut channel,
    machine: &mut machine,
    state: &mut state,
    deadline: Instant::now() + Duration::from_secs(1),
  };
  assert!(matches!(
    context.send_data(b"payload").await,
    Err(HandlerError::Transport(assuan_transport::TransportError::Timeout))
  ));
  assert_eq!(machine.state(), ServerState::Invalid);
  let mut prefix = Vec::new();
  peer.read_to_end(&mut prefix).await.unwrap();
  assert_eq!(prefix, b"D");
}

#[tokio::test]
async fn illegal_output_closes_without_sending_and_debug_hides_state() {
  let (local, mut peer) = tokio::io::duplex(16);
  let mut channel = Channel::new(Stream::new(local));
  let mut machine = ServerMachine::new();
  let mut state = String::from("SECRET_MARKER");
  let mut context = CommandContext {
    channel: &mut channel,
    machine: &mut machine,
    state: &mut state,
    deadline: Instant::now() + Duration::from_secs(1),
  };
  assert_eq!(context.state(), "SECRET_MARKER");
  assert!(!format!("{context:?}").contains("SECRET_MARKER"));
  assert!(matches!(context.send_data(b"payload").await, Err(HandlerError::State(_))));
  assert_eq!(machine.state(), ServerState::Invalid);
  let mut bytes = Vec::new();
  peer.read_to_end(&mut bytes).await.unwrap();
  assert!(bytes.is_empty());
}

#[tokio::test]
async fn dropping_unpolled_send_preserves_session() {
  let (local, _peer) = tokio::io::duplex(16);
  let mut channel = Channel::new(Stream::new(local));
  let mut machine = machine();
  let mut state = ();
  let mut context = CommandContext {
    channel: &mut channel,
    machine: &mut machine,
    state: &mut state,
    deadline: Instant::now() + Duration::from_secs(1),
  };
  drop(context.send_data(b"unused"));
  context.send_data(b"ok").await.unwrap();
  assert_eq!(machine.state(), ServerState::Handler);
}
