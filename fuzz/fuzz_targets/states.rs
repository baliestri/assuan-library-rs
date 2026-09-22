#![no_main]

use assuan_protocol::{
  ClientMachine, ClientState, LineKind, RequestKind, ServerMachine, ServerState,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
  let mut client = ClientMachine::new();
  let mut server = ServerMachine::new();
  for byte in bytes.iter().take(4096) {
    let client_terminal = matches!(client.state(), ClientState::Invalid | ClientState::Closed);
    let server_terminal = matches!(server.state(), ServerState::Invalid | ServerState::Closed);
    let line = [
      LineKind::Ok,
      LineKind::Err,
      LineKind::Data,
      LineKind::Status,
      LineKind::Inquire,
      LineKind::End,
      LineKind::Comment,
      LineKind::Empty,
    ][usize::from(byte & 7)];
    match byte >> 4 {
      0..=7 => {
        let _ = client.receive(line);
        let _ = server.send_response(line);
      }
      8 => {
        let _ = client.begin_command();
        let _ = server.receive_request(RequestKind::Command);
      }
      9 => {
        let _ = client.finish_inquiry(false);
        let _ = server.receive_request(RequestKind::End);
      }
      10 => {
        let _ = client.finish_inquiry(true);
        let _ = server.receive_request(RequestKind::Can);
      }
      11 => {
        client.invalidate();
        server.invalidate();
      }
      12 => {
        client.close();
        server.close();
      }
      _ => {
        let request =
          [RequestKind::Data, RequestKind::Comment, RequestKind::Empty][usize::from(byte % 3)];
        let _ = server.receive_request(request);
      }
    }
    if client_terminal {
      assert!(matches!(client.state(), ClientState::Invalid | ClientState::Closed));
    }
    if server_terminal {
      assert!(matches!(server.state(), ServerState::Invalid | ServerState::Closed));
    }
  }
  return;
});
