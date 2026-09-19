use crate::ClientError;
use assuan_protocol::{
  ClientMachine, ClientState, LineKind, Sensitivity, ServerLine, StateError, decode_data_in_place,
  parse_server_line,
};
use assuan_transport::Channel;
use std::ops::Range;
use tokio::time::Instant;

#[derive(Debug)]
pub(crate) struct SessionCore {
  pub channel: Channel,
  pub machine: ClientMachine,
  pub io_uncertain: bool,
  pub deadline: Instant,
  pub sensitivity: Sensitivity,
  pub inquiry_timeout: std::time::Duration,
  pub max_inquiry_bytes: usize,
}

pub(crate) struct IoGuard<'a>(&'a mut bool);

impl<'a> IoGuard<'a> {
  pub fn begin(marker: &'a mut bool) -> Self {
    *marker = true;
    return Self(marker);
  }

  pub fn complete(self) {
    *self.0 = false;
  }
}

impl Drop for IoGuard<'_> {
  fn drop(&mut self) {
    // Only complete can certify that the whole operation finished.
  }
}

pub(crate) struct Received {
  pub kind: LineKind,
  pub code: Option<u32>,
  pub payload: Range<usize>,
  pub keyword: Range<usize>,
}

impl SessionCore {
  pub fn invalidate(&mut self) {
    self.machine.invalidate();
    self.channel.close();
  }

  pub fn check(&mut self) -> Result<(), ClientError> {
    if self.io_uncertain
      || matches!(self.machine.state(), ClientState::Invalid | ClientState::Closed)
    {
      self.invalidate();
      return Err(StateError::Unusable.into());
    }
    return Ok(());
  }

  pub async fn read(&mut self) -> Result<Received, ClientError> {
    self.check()?;
    let guard = IoGuard::begin(&mut self.io_uncertain);
    let result = match self.channel.read_line(self.deadline, self.sensitivity).await {
      Ok(line) => classify(line, &mut self.machine),
      Err(error) => Err(error.into()),
    };
    match result {
      Ok(received) => {
        guard.complete();
        return Ok(received);
      }
      Err(error) => {
        drop(guard);
        self.invalidate();
        return Err(error);
      }
    }
  }

  pub async fn write(&mut self, bytes: &[u8]) -> Result<(), ClientError> {
    return self.write_at(bytes, self.deadline, self.sensitivity).await;
  }

  pub async fn write_at(
    &mut self,
    bytes: &[u8],
    deadline: Instant,
    sensitivity: Sensitivity,
  ) -> Result<(), ClientError> {
    self.check()?;
    let guard = IoGuard::begin(&mut self.io_uncertain);
    let result = self.channel.write_line(bytes, deadline, sensitivity).await;
    match result {
      Ok(()) => guard.complete(),
      Err(error) => {
        drop(guard);
        self.invalidate();
        return Err(error.into());
      }
    }
    return Ok(());
  }
}

fn classify(line: &mut [u8], machine: &mut ClientMachine) -> Result<Received, ClientError> {
  let parsed = parse_server_line(line)?;
  let kind = parsed.kind();
  let mut received = Received {
    kind,
    code: None,
    payload: 0..0,
    keyword: 0..0,
  };
  match parsed {
    ServerLine::Ok(text) | ServerLine::Comment(text) | ServerLine::Data(text) => {
      received.payload = line.len() - text.len()..line.len();
    }
    ServerLine::Err {
      code,
      text,
    } => {
      received.code = Some(code);
      received.payload = line.len() - text.len()..line.len();
    }
    ServerLine::Status {
      keyword,
      args,
    } => {
      received.keyword = 2..2 + keyword.len();
      received.payload = line.len() - args.len()..line.len();
    }
    ServerLine::Inquire {
      keyword,
      args,
    } => {
      received.keyword = 8..8 + keyword.len();
      received.payload = line.len() - args.len()..line.len();
    }
    ServerLine::End | ServerLine::Empty => {}
  }
  if kind == LineKind::Data {
    let decoded = decode_data_in_place(&mut line[received.payload.clone()])?;
    received.payload.end = received.payload.start + decoded;
  }
  if let Err(error) = machine.receive(kind) {
    if error == StateError::GreetingRejected {
      return Err(ClientError::GreetingRejected {
        code: received.code.unwrap_or_default(),
      });
    }
    return Err(error.into());
  }
  return Ok(received);
}
