//! Exhaustive event legality and end-to-end session transition regressions.

use assuan_protocol::{
  ClientMachine, ClientState as C, LineKind as L, RequestKind as R, ServerMachine,
  ServerState as S, StateError, parse_server_line,
};

const LINES: [L; 8] = [L::Ok, L::Err, L::Data, L::Status, L::Inquire, L::End, L::Comment, L::Empty];
const REQUESTS: [R; 6] = [R::Command, R::Data, R::End, R::Can, R::Comment, R::Empty];

fn client_in(state: C) -> ClientMachine {
  let mut client = ClientMachine::new();
  if state == C::Greeting {
    return client;
  }
  if state == C::Invalid {
    client.invalidate();
    return client;
  }
  if state == C::Closed {
    client.close();
    return client;
  }
  client.receive(L::Ok).unwrap();
  if state == C::Ready {
    return client;
  }
  client.begin_command().unwrap();
  if state == C::Command {
    return client;
  }
  client.receive(L::Inquire).unwrap();
  if state == C::AwaitFinal {
    client.finish_inquiry(true).unwrap();
  }
  return client;
}

fn server_in(state: S) -> ServerMachine {
  let mut server = ServerMachine::new();
  if state == S::Greeting {
    return server;
  }
  if state == S::Invalid {
    server.invalidate();
    return server;
  }
  if state == S::Closed {
    server.close();
    return server;
  }
  server.send_response(L::Ok).unwrap();
  if state == S::Ready {
    return server;
  }
  server.receive_request(R::Command).unwrap();
  if state == S::Handler {
    return server;
  }
  server.send_response(L::Inquire).unwrap();
  if state == S::AwaitFinal {
    server.receive_request(R::Can).unwrap();
  }
  return server;
}

#[test]
fn client_checks_every_response_in_every_state() {
  // Columns: OK, ERR, D, S, INQUIRE, END, comment, empty.
  for (state, accepted) in [
    (C::Greeting, [true, false, false, true, true, false, true, true]),
    (C::Ready, [false, false, false, false, false, false, true, true]),
    (C::Command, [true, true, true, true, true, true, true, true]),
    (C::Inquiry, [false, false, false, true, false, false, true, true]),
    (C::AwaitFinal, [true, true, false, true, false, false, true, true]),
    (C::Invalid, [false; 8]),
    (C::Closed, [false; 8]),
  ] {
    for (event, allowed) in LINES.into_iter().zip(accepted) {
      let mut client = client_in(state);
      assert_eq!(client.receive(event).is_ok(), allowed, "{state:?} / {event:?}");
      if !allowed {
        assert_eq!(
          client.state(),
          if state == C::Closed {
            C::Closed
          } else {
            C::Invalid
          }
        );
      }
    }
  }
}

#[test]
fn server_checks_every_response_in_every_state() {
  for (state, accepted) in [
    (S::Greeting, [true, true, false, true, true, false, true, true]),
    (S::Ready, [false, false, false, false, false, false, true, true]),
    (S::Handler, [true, true, true, true, true, true, true, true]),
    (S::Inquiry, [false, false, false, true, false, false, true, true]),
    (S::AwaitFinal, [true, true, false, true, false, false, true, true]),
    (S::Invalid, [false; 8]),
    (S::Closed, [false; 8]),
  ] {
    for (event, allowed) in LINES.into_iter().zip(accepted) {
      let mut server = server_in(state);
      assert_eq!(server.send_response(event).is_ok(), allowed, "{state:?} / {event:?}");
      if !allowed {
        assert_eq!(
          server.state(),
          if state == S::Closed {
            S::Closed
          } else {
            S::Invalid
          }
        );
      }
    }
  }
}

#[test]
fn server_checks_every_request_in_every_state() {
  // Columns: command, D, END, CAN, comment, empty.
  for (state, accepted) in [
    (S::Greeting, [false, false, false, false, true, true]),
    (S::Ready, [true, false, false, false, true, true]),
    (S::Handler, [false, false, false, false, true, true]),
    (S::Inquiry, [false, true, true, true, true, true]),
    (S::AwaitFinal, [false, false, false, false, true, true]),
    (S::Invalid, [false; 6]),
    (S::Closed, [false; 6]),
  ] {
    for (event, allowed) in REQUESTS.into_iter().zip(accepted) {
      let mut server = server_in(state);
      assert_eq!(server.receive_request(event).is_ok(), allowed, "{state:?} / {event:?}");
      if !allowed {
        assert_eq!(
          server.state(),
          if state == S::Closed {
            S::Closed
          } else {
            S::Invalid
          }
        );
      }
    }
  }
}

#[test]
fn greeting_inquiries_resume_the_greeting_then_allow_commands() {
  let mut client = ClientMachine::default();
  let mut server = ServerMachine::default();
  for _ in 0..2 {
    for event in [L::Comment, L::Status, L::Inquire] {
      server.send_response(event).unwrap();
      client.receive(event).unwrap();
    }
    server.receive_request(R::Data).unwrap();
    server.receive_request(R::End).unwrap();
    client.finish_inquiry(false).unwrap();
    assert_eq!(client.state(), C::Greeting);
    assert_eq!(server.state(), S::Greeting);
    assert_eq!(client.begin_command(), Err(StateError::NotReady));
  }
  server.send_response(L::Ok).unwrap();
  client.receive(L::Ok).unwrap();
  assert_eq!(server.state(), S::Ready);
  assert_eq!(client.state(), C::Ready);
}

#[test]
fn cancelled_inquiry_requires_a_final_and_preserves_reuse_after_err() {
  let mut client = client_in(C::Command);
  let mut server = server_in(S::Handler);
  server.send_response(L::Inquire).unwrap();
  client.receive(L::Inquire).unwrap();
  server.receive_request(R::Can).unwrap();
  client.finish_inquiry(true).unwrap();
  assert_eq!(client.state(), C::AwaitFinal);
  assert_eq!(server.state(), S::AwaitFinal);
  assert_eq!(client.begin_command(), Err(StateError::NotReady));
  server.send_response(L::Status).unwrap();
  client.receive(L::Status).unwrap();
  server.send_response(L::Err).unwrap();
  client.receive(L::Err).unwrap();
  client.begin_command().unwrap();
  server.receive_request(R::Command).unwrap();
  assert_eq!(client.state(), C::Command);
  assert_eq!(server.state(), S::Handler);
}

#[test]
fn greeting_rejection_also_applies_after_inquiry_cancellation() {
  for cancel in [false, true] {
    let mut client = ClientMachine::new();
    let mut server = ServerMachine::new();
    if cancel {
      server.send_response(L::Inquire).unwrap();
      client.receive(L::Inquire).unwrap();
      server.receive_request(R::Can).unwrap();
      client.finish_inquiry(true).unwrap();
    }
    server.send_response(L::Err).unwrap();
    assert_eq!(client.receive(L::Err), Err(StateError::GreetingRejected));
    assert_eq!(client.state(), C::Invalid);
    assert_eq!(server.state(), S::Invalid);
    assert_eq!(client.begin_command(), Err(StateError::Unusable));
  }
}

#[test]
fn partial_end_does_not_release_transaction_and_final_cannot_repeat() {
  let mut client = client_in(C::Command);
  let mut server = server_in(S::Handler);
  for event in [L::Data, L::End, L::Status, L::Data, L::Comment, L::Empty] {
    server.send_response(event).unwrap();
    client.receive(event).unwrap();
    assert_eq!(client.state(), C::Command);
    assert_eq!(server.state(), S::Handler);
  }
  server.send_response(L::Ok).unwrap();
  client.receive(L::Ok).unwrap();
  assert_eq!(server.send_response(L::Ok), Err(StateError::UnexpectedEvent));
  assert_eq!(client.receive(L::Ok), Err(StateError::UnexpectedEvent));
}

#[test]
fn rejected_local_operations_preserve_active_phase() {
  for state in [C::Greeting, C::Command, C::Inquiry, C::AwaitFinal] {
    let mut client = client_in(state);
    assert_eq!(client.begin_command(), Err(StateError::NotReady));
    assert_eq!(client.state(), state);
  }
  for state in [C::Greeting, C::Ready, C::Command, C::AwaitFinal] {
    let mut client = client_in(state);
    assert_eq!(client.finish_inquiry(false), Err(StateError::NotReady));
    assert_eq!(client.state(), state);
  }
}

#[test]
fn command_inquiry_end_resumes_handler_without_finishing() {
  let mut client = client_in(C::Inquiry);
  let mut server = server_in(S::Inquiry);
  client.finish_inquiry(false).unwrap();
  server.receive_request(R::End).unwrap();
  assert_eq!(client.state(), C::Command);
  assert_eq!(server.state(), S::Handler);
}

#[test]
fn response_categories_match_wire_variants() {
  for (wire, expected) in [
    (b"OK".as_slice(), L::Ok),
    (b"ERR 1", L::Err),
    (b"D x", L::Data),
    (b"S foo", L::Status),
    (b"INQUIRE foo", L::Inquire),
    (b"END", L::End),
    (b"# comment", L::Comment),
    (b"", L::Empty),
  ] {
    assert_eq!(parse_server_line(wire).unwrap().kind(), expected);
  }
}

#[test]
fn invalidation_and_closure_are_terminal() {
  let mut client = client_in(C::Inquiry);
  let mut server = server_in(S::Inquiry);
  client.invalidate();
  server.invalidate();
  assert_eq!(client.finish_inquiry(true), Err(StateError::Unusable));
  assert_eq!(server.receive_request(R::Can), Err(StateError::Unusable));
  client.close();
  server.close();
  client.invalidate();
  server.invalidate();
  assert_eq!(client.state(), C::Closed);
  assert_eq!(server.state(), S::Closed);
}
