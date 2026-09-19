//! Client and server session transition contracts.

#[test]
fn remote_error_completes_a_command() {
  use assuan_protocol::{ClientMachine, ClientState, LineKind};
  let mut machine = ClientMachine::new();
  machine.receive(LineKind::Ok).unwrap();
  machine.begin_command().unwrap();
  machine.receive(LineKind::Err).unwrap();
  assert_eq!(machine.state(), ClientState::Ready);
}
