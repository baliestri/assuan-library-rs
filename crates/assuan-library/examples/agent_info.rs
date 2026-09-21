//! Discovers an existing agent without starting a service.

use std::{error::Error, path::PathBuf};

use assuan_library::{AgentLocator, Client, ClientOptions, CollectLimits, Command, ConnectOptions};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
  let mut arguments = std::env::args_os().skip(1);
  let executable = PathBuf::from(arguments.next().unwrap_or_else(|| return "gpgconf".into()));
  let homedir = arguments.next().map(PathBuf::from);
  let resolved = AgentLocator::new(executable, homedir).resolve().await?;
  let stream = resolved.connect(&ConnectOptions::default()).await?;
  let mut client = Client::from_stream(stream, ClientOptions::default()).await?;
  let response =
    client.collect(Command::new("GETINFO", b"version")?, CollectLimits::default()).await?;
  println!("GETINFO version completed with {} public data bytes", response.data().len());
  client.collect(Command::new("BYE", b"")?, CollectLimits::default()).await?;
  return Ok(());
}
