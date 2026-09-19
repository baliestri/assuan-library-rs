#![doc = include_str!("../README.md")]

mod context;
mod error;
mod handler;
mod options;
mod registry;

pub use context::CommandContext;
pub use error::{HandlerError, RegistryError};
pub use handler::{Handler, HandlerFuture};
pub use registry::{Registry, handler};
