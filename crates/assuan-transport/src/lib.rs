#![doc = include_str!("../README.md")]

mod endpoint;
mod error;
mod listener;
mod stream;
mod tcp;

pub use endpoint::{ConnectOptions, Endpoint, ListenOptions, LocalAccess, PeerIdentity};
pub use error::{DiscoveryError, TransportError};
pub use listener::{Accepted, Acceptor, IoFuture, Listener};
pub use stream::{IoStream, Stream};
pub use tcp::connect;

#[cfg(unix)]
mod unix;
