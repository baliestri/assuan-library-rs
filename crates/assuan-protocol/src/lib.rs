#![doc = include_str!("../README.md")]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod command;
mod error;
mod escape;
mod limits;
mod message;
mod secret;

pub use command::Command;
pub use error::{LimitError, ProtocolError};
pub use escape::decode_data_in_place;
pub use message::{ServerLine, parse_server_line};
pub use secret::{PayloadRef, SecretBytes, SecretRef, Sensitivity};
