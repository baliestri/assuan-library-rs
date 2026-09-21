//! Sends a public command to the loopback example server.

use std::{
  error::Error,
  net::{Ipv4Addr, SocketAddr},
};

use assuan_library::{Client, ClientOptions, CollectLimits, Command, Endpoint};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
  let port: u16 = std::env::args().nth(1).unwrap_or_else(|| return "9000".into()).parse()?;
  let endpoint = Endpoint::Tcp(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
  let mut client = Client::connect(&endpoint, ClientOptions::default()).await?;
  let response = client.collect(Command::new("ECHO", b"hello")?, CollectLimits::default()).await?;
  println!("Received {} public data bytes", response.data().len());
  client.collect(Command::new("BYE", b"")?, CollectLimits::default()).await?;
  return Ok(());
}
