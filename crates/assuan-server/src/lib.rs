#![doc = include_str!("../README.md")]

mod context;
mod error;
mod handler;
mod hooks;
mod inquiry;
mod options;
mod registry;
mod session;

pub use context::CommandContext;
pub use error::{HandlerError, RegistryError, ServerError};
pub use handler::{Handler, HandlerFuture};
pub use hooks::{DefaultHooks, HookFuture, OptionRequest, SessionEnd, SessionHooks};
pub use inquiry::{InquiryOutcome, ServerInquiry};
pub use options::ServerOptions;
pub use registry::{Registry, handler};
pub use session::Session;
