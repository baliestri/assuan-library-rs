#![doc = include_str!("../README.md")]

// Macro expansions use this explicit self-alias in library code, examples
// and doctests. External consumers still resolve their Cargo dependency alias.
extern crate self as assuan_library;

/// Asynchronous client transactions, collection and interactive inquiries.
#[cfg(feature = "client")]
pub use assuan_client as client;
#[cfg(all(feature = "macros", feature = "server"))]
pub use assuan_macros::assuan_command;
#[cfg(feature = "macros")]
pub use assuan_macros::command;
#[cfg(all(feature = "macros", feature = "sexpr"))]
pub use assuan_macros::sexpr;
/// Runtime-independent protocol parsing, encoding and protected data.
pub use assuan_protocol as protocol;
/// Typed handlers, session hooks and bounded concurrent serving.
#[cfg(feature = "server")]
pub use assuan_server as server;
/// Bounded canonical S-expression parsing and encoding.
#[cfg(feature = "sexpr")]
pub use assuan_sexpr as sexpr;
/// Standard and custom asynchronous transports and agent discovery.
pub use assuan_transport as transport;
#[cfg(feature = "client")]
pub use client::{
  Client, ClientError, ClientFuture, ClientOptions, CollectLimits, Event, GreetingHandler,
  Handshake, Inquiry, Response, SecretResponse, Transaction,
};
pub use protocol::{
  ClientMachine, ClientState, Command, LimitError, LineBuffer, LineKind, MAX_LINE_BYTES,
  PayloadRef, ProtocolError, RequestKind, SecretBytes, SecretRef, Sensitivity, ServerLine,
  ServerMachine, ServerState, StateError,
};
#[cfg(feature = "server")]
pub use server::{
  CommandContext, DefaultHooks, Handler, HandlerError, HandlerFuture, HookFuture, InquiryOutcome,
  OptionRequest, Registry, RegistryError, Server, ServerError, ServerInquiry, ServerOptions,
  Session, SessionEnd, SessionHooks, handler,
};
#[cfg(feature = "sexpr")]
pub use sexpr::{
  EncodeError, MAX_NESTING_DEPTH, OwnedSexpr, ParseError, ParseErrorKind, ParseLimits, Sexpr,
};
pub use transport::{
  Accepted, Acceptor, AgentLocator, Channel, ConnectOptions, DiscoveryError, Endpoint, IoFuture,
  IoStream, ListenOptions, Listener, LocalAccess, PeerIdentity, ResolvedEndpoint, Stream,
  TransportError, connect,
};

#[cfg(all(test, feature = "macros"))]
mod tests {
  #[test]
  fn literal_resolves_inside_the_facade_itself() {
    let command = crate::command!("GETINFO version");
    assert_eq!(command.name(), "GETINFO");
  }

  #[cfg(feature = "server")]
  #[test]
  fn attribute_resolves_inside_the_facade_itself() {
    #[crate::assuan_command("INTERNAL", "Uses the current crate")]
    async fn internal(
      _: crate::Command<'_>,
      context: &mut crate::CommandContext<'_>,
    ) -> Result<(), crate::HandlerError> {
      return context.send_data(b"public").await;
    }
    let mut registry = crate::Registry::<()>::new();
    registry.register(internal).unwrap();
    assert!(registry.get("INTERNAL").is_some());
  }
}
