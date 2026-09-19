# assuan-server

Typed asynchronous Assuan handler contracts and an explicit command registry.
This crate currently provides the handler foundation; the session runner,
built-in dispatch, hooks, inquiries, and concurrent serving are subsequent
implementation steps.

Implement `Handler<S>` directly or use `handler` to adapt a function or closure.
Each session owns its application state `S`; handlers borrow it exclusively
through `CommandContext`. Command arguments remain borrowed across awaits.
Handlers may stream data, but only the session runner may send the final
response. A handler invocation uses one boxed future.

Registration validates command syntax, reserves `NOP`, `BYE`, `HELP`, `RESET`,
and `OPTION`, and rejects duplicate names without replacing the original.
Names are case-sensitive. Descriptions are public HELP text and cannot contain
CR, LF, or NUL.

`send_data` escapes binary bytes, respects the fixed 1,000-byte wire limit,
and uses one absolute deadline for every chunk. Cancellation or failure
invalidates the protocol state and closes the stream. Intermediate buffers
are wiped; the application remains responsible for its input and state.
Diagnostic formatting does not expose handler captures, state, or remote
error text. Remote error text is intended for the peer and must contain only
public information.

See `handler` for a complete registration example.
