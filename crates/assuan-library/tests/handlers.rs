//! Macro and ordinary handlers sharing real session state.

use std::{cell::Cell, sync::Arc, time::Duration};

use assuan_library::{
  Accepted, Client, ClientOptions, CollectLimits, Command, CommandContext, DefaultHooks,
  HandlerError, HandlerFuture, Registry, ServerOptions, Session, Stream, assuan_command, handler,
};

#[derive(Default)]
struct Counter(Cell<u32>);

#[assuan_command("INCREMENT", "Increments the session counter")]
async fn increment(
  _: Command<'_>,
  context: &mut CommandContext<'_, Counter>,
) -> Result<(), HandlerError> {
  context.state().0.set(context.state().0.get() + 1);
  tokio::task::yield_now().await;
  return Ok(());
}

fn read<'a>(_: Command<'a>, mut context: CommandContext<'a, Counter>) -> HandlerFuture<'a> {
  return Box::pin(async move {
    let count = context.state().0.get().to_string();
    return context.send_data(count.as_bytes()).await;
  });
}

async fn exercise() {
  let mut registry = Registry::<Counter>::new();
  registry.register(increment).unwrap();
  registry.register(handler("READ", "Reads the same counter", read).unwrap()).unwrap();
  let (server_io, client_io) = tokio::io::duplex(1024);
  let session = Session::new(
    Accepted {
      stream: Stream::new(server_io),
      peer: None,
    },
    Counter::default(),
    Arc::new(registry),
    Arc::new(DefaultHooks::default()),
    ServerOptions::default(),
  );
  let serving = tokio::spawn(session.run());
  let mut client =
    Client::from_stream(Stream::new(client_io), ClientOptions::default()).await.unwrap();
  for expected in [b"1", b"2"] {
    client
      .collect(Command::new("INCREMENT", b"").unwrap(), CollectLimits::default())
      .await
      .unwrap();
    let response =
      client.collect(Command::new("READ", b"").unwrap(), CollectLimits::default()).await.unwrap();
    assert_eq!(response.data(), expected);
  }
  client.collect(Command::new("BYE", b"").unwrap(), CollectLimits::default()).await.unwrap();
  serving.await.unwrap().unwrap();
}

#[tokio::test]
async fn macro_and_plain_handlers_share_one_session_and_concrete_state() {
  tokio::time::timeout(Duration::from_secs(5), exercise()).await.unwrap();
}
