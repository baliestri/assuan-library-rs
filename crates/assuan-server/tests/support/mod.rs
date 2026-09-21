use std::sync::Arc;

use assuan_server::{DefaultHooks, Registry, ServerError, ServerOptions, Session, SessionHooks};
use assuan_transport::{Accepted, Stream};
use tokio::{
  io::{AsyncReadExt, DuplexStream},
  task::JoinHandle,
};

pub fn start<S: Send + 'static>(
  state: S,
  registry: Registry<S>,
  hooks: Arc<dyn SessionHooks<S>>,
  options: ServerOptions,
) -> (DuplexStream, JoinHandle<Result<(), ServerError>>) {
  let (io, peer) = tokio::io::duplex(2048);
  let session = Session::new(
    Accepted {
      stream: Stream::new(io),
      peer: None,
    },
    state,
    Arc::new(registry),
    hooks,
    options,
  );
  return (peer, tokio::spawn(session.run()));
}

pub fn defaults() -> Arc<DefaultHooks> {
  return Arc::new(DefaultHooks::default());
}

pub async fn expect(peer: &mut DuplexStream, expected: &[u8]) {
  let mut bytes = vec![0; expected.len()];
  peer.read_exact(&mut bytes).await.unwrap();
  assert_eq!(bytes, expected);
}

pub async fn eof(peer: &mut DuplexStream) {
  let mut rest = Vec::new();
  peer.read_to_end(&mut rest).await.unwrap();
  assert!(rest.is_empty(), "unexpected wire bytes: {rest:?}");
}
