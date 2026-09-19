#![doc = include_str!("../README.md")]

mod client;
mod error;
mod options;
mod session;
mod transaction;

pub use assuan_protocol::PayloadRef;
pub use client::Client;
pub use error::ClientError;
pub use options::ClientOptions;
pub use transaction::{Event, Transaction};

mod handshake;
mod inquiry;
pub use handshake::{ClientFuture, GreetingHandler, Handshake};
pub use inquiry::Inquiry;
