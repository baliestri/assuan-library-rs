//! Runs a bounded ECHO server bound explicitly to IPv4 loopback.

use std::{
  error::Error,
  net::{Ipv4Addr, SocketAddr},
};

use assuan_library::{
  Command, CommandContext, Endpoint, HandlerError, ListenOptions, Listener, Server, ServerOptions,
  assuan_command,
};

#[assuan_command("ECHO", "Returns public command arguments")]
async fn echo(command: Command<'_>, context: &mut CommandContext<'_>) -> Result<(), HandlerError> {
  return context.send_data(command.args()).await;
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
  let port: u16 = std::env::args().nth(1).unwrap_or_else(|| return "9000".into()).parse()?;
  let endpoint = Endpoint::Tcp(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
  let listener = Listener::bind(&endpoint, &ListenOptions::default()).await?;
  println!("Listening on {:?}; press Ctrl+C to stop", listener.endpoint());
  let mut server = Server::new(|| (), ServerOptions::default());
  server.register(echo)?;
  server
    .serve(listener, async {
      if tokio::signal::ctrl_c().await.is_err() {
        eprintln!("Could not wait for shutdown signal");
      }
    })
    .await?;
  return Ok(());
}
