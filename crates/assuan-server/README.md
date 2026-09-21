# assuan-server

Typed asynchronous Assuan sessions, streaming inquiries, application hooks,
and an explicit command registry.

`Session::new` owns an `Accepted` stream, typed state, shared registry, hooks,
and `ServerOptions`. This works with standard listeners and custom streams.
`run` authenticates before greeting OK and processes commands serially.
OPTION and RESET dispatch to typed hooks. NOP succeeds without changing state;
BYE sends OK before closing. HELP lists built-ins and registered handlers as
comment lines, splitting long descriptions on UTF-8 boundaries within the
wire limit. Registry iteration order is unspecified.

`Server::new` accepts a state factory and options. Register handlers and set
hooks before calling `Server::serve` with a standard `Listener` or custom
`Acceptor`. A custom acceptor may leave `endpoint()` at its default None.

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

## Session policy

`DefaultHooks` accepts connections already admitted by the listener and rejects
unknown options. It performs no additional authentication. TCP has no OS peer
identity; applications needing authentication must install `SessionHooks`.
Hooks can inspect `CommandContext::peer` and borrow session state. RESET keeps
the library's authentication flag; the reset hook must also preserve the
application's authentication fields while clearing its transient fields.

Success produces exactly one OK. A validated remote error produces one ERR;
an internal error produces only generic text. Both ordinary command failures
leave the session reusable. Protocol failures, invalid remote text, unfinished
inquiries, and uncertain I/O close the stream without a final response.
Numeric error codes follow
[libgpg-error](https://github.com/gpg/libgpg-error/blob/master/src/err-codes.h.in).
OPTION parsing follows the
[Assuan request syntax](https://www.gnupg.org/documentation/manuals/assuan/Client-requests.html).

One fixed, protected command buffer copies at most one 1,000-byte wire line.
Handlers borrow this buffer across awaits while using the mutable channel.
Data returned by `ServerInquiry::next` borrows channel storage and is decoded
in place. Choose `Sensitivity` before starting the inquiry. Consume until None,
then call `finish` to distinguish END from CAN; CAN leaves the application
decision to the handler. Finish does not drain unread data. Abandoning an
inquiry closes the stream, and forgetting it prevents session reuse.

Default total budgets are 120 seconds for authentication/greeting, 300 seconds
per command, 120 seconds per inquiry, 300 seconds between commands, and 30
seconds for the close hook. An inquiry cannot extend its enclosing deadline.
Each inquiry accepts at most one MiB of decoded bytes. All limits must be
positive. Comments, fragments and data chunks do not restart deadlines.

The close hook runs after disconnection or a returned error, once the channel
is closed. Its failure never replaces an earlier session error. Secondary
failure logging uses the `log` facade and emits only a category. Dropping or
aborting the run future, or unwinding an application panic, closes the owned
stream but cannot await this hook. No cleanup task is spawned in Drop; keep
essential synchronous resource cleanup in your state types' destructors.

## Concurrent serving

The default limit is 128 sessions. A permit is acquired before polling accept
and retained through authentication, commands, and the close hook. A stalled
session does not block another when capacity remains. At capacity the library
does not create pending session tasks; the transport controls its own backlog.
Completed tasks are reaped while serving. Session failures and task panics are
isolated; library diagnostics log only their categories.

When the shutdown future resolves, the acceptor is dropped and existing
sessions receive one shared grace period (30 seconds by default). Sessions can
finish work or issue BYE during that period. Remaining tasks are then aborted
and joined. Async deadlines require cooperative code; they cannot preempt a
handler that blocks a runtime thread indefinitely. Dropping serve also aborts
its tasks, but cannot await cleanup.

`ServerOptions::with_max_sessions`, `with_max_inquiry_bytes`, and the
`with_*_timeout` builders validate all configured limits. Direct field edits
are checked again before serving or running a session.

```no_run
# async fn run(shutdown: impl std::future::Future<Output = ()> + Send)
#   -> Result<(), Box<dyn std::error::Error>> {
use assuan_protocol::Command;
use assuan_server::{CommandContext, HandlerFuture, Server, ServerOptions, handler};
use assuan_transport::{Endpoint, ListenOptions, Listener};

fn echo<'a>(command: Command<'a>, mut ctx: CommandContext<'a>) -> HandlerFuture<'a> {
  return Box::pin(async move { return ctx.send_data(command.args()).await; });
}

let options = ServerOptions::default().with_max_sessions(128)?;
let mut server = Server::new(|| (), options);
server.register(handler("ECHO", "Returns the original arguments", echo)?)?;
let endpoint = Endpoint::Tcp("127.0.0.1:9000".parse()?);
let listener = Listener::bind(&endpoint, &ListenOptions::default()).await?;
server.serve(listener, shutdown).await?;
# return Ok(());
# }
```

Accept errors stop serving and are returned after draining existing sessions.
Transport-specific filesystem cleanup remains explicit: dropping a Unix
listener closes its socket but does not unlink its pathname.
