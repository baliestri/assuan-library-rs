# Security and ownership

## Trust boundaries

Assuan framing and protocol validation do not authenticate an application or
encrypt its traffic. Choose a transport policy before exchanging secrets.

| Transport | Built-in policy | Application responsibility |
| --- | --- | --- |
| TCP (Windows, Linux, macOS) | Ordered byte transport with deadlines | Authentication and encryption; loopback alone is not authentication |
| Unix domain socket | Private current-user-owned parent, restricted socket permissions, matching peer UID | Trust same-user and privileged processes; call explicit listener cleanup when appropriate |
| Windows named pipe | Local-only byte pipe with a protected current-user DACL | Application authorization; no inferred `PeerIdentity` |
| Custom `Stream` / `Acceptor` | Delegation to the supplied implementation | Transport authentication, encryption, cancellation and truthful optional peer identity |

There is no built-in TLS backend. A compatible externally secured byte stream
can be passed to `Stream::new`. Server `DefaultHooks` add no authentication;
implement application checks in hooks before admitting commands. Unix pathname
checks cannot prevent concurrent mutations by trusted same-user/root processes.
Dropping a Unix listener closes it but does not unlink its pathname; `cleanup`
checks the recorded identities before removal.

## Protected storage and borrowed input

`SecretBytes` owns fixed-capacity heap storage, initializes it before use,
rejects growth past capacity, and wipes its entire allocation on clear and
normal drop. `SecretRef` is an explicit borrowed view, not an owner. A borrow
prevents mutation of its owner while live; it does not erase caller memory.
Pure protocol parsers and S-expression atoms likewise borrow input without
assuming responsibility for its cleanup.

The asynchronous channel wipes its line, read-ahead and outgoing buffers
regardless of sensitivity. Read-ahead can contain secrets before classification.
Streaming events borrow these buffers and must be released before the next
mutable session operation. Select `Sensitivity::Secret` before sending a command
when its response should require explicit exposure.

`collect_secret` copies decoded data directly into fixed protected storage.
Status fields and comments are retained in ordinary allocations, even in a
`SecretResponse`: stream the response if those fields may contain secrets.
Public collection and owned S-expressions use ordinary memory. Collection
limits bound retained bytes and events, not allocation overhead. Configure
finite limits suitable for the application instead of trusting peer sizes.

Wiping cannot erase copies made by callers, compiler/stack moves, operating
system buffers, swap, dumps or remote peers. There is no memory locking.
`mem::forget`, process termination and aborts can prevent destructors from
running. Compile-time macro literals are embedded in programs and may appear
in compiler diagnostics; never place real secrets in them.

## Cancellation and connection reuse

Assuan commands run serially per connection. Dropping unfinished transactions
or inquiries abandons the session. Cancelling an active I/O operation can leave
partial delivery; the client/server invalidates uncertain state instead of
retrying. An unpolled future has not started its operation. Leaking a guard
does not reset the protocol state or make the connection reusable.

Inquiry `CAN` is an on-wire cancellation followed by a required final `OK` or
`ERR`; it is different from dropping the inquiry. A command's final `ERR` is a
typed remote failure and normally permits reuse. A rejected greeting does not.
Transaction and handshake `finish` require completion already observed and do
not drain the network. Server inquiry `finish` similarly requires its terminator.

Dropping serving futures cannot await graceful shutdown hooks. Use the server's
shutdown signal and configured grace period when hooks must run. Deadlines,
session limits and bounded inquiries restrict resource use, not application
work performed outside the library.

## Diagnostics and discovery

Library diagnostics omit payloads, received status keywords, nonces and complete
external lines. Explicit error sources and exposed data remain under caller
control: do not recursively log external errors or payloads without review.
Collected-response `Debug` prints counts, never retained contents.

`AgentLocator` runs the explicitly selected `gpgconf` executable with a bounded
output budget and deadline. Select a trusted executable and homedir. Discovery
does not launch an agent. Use `ResolvedEndpoint::connect` so native Windows
nonce authentication occurs before the Assuan greeting; connecting to its
address alone skips that handshake. Native socket-file handling does not imply
support for Cygwin socket formats.

See [protocol compatibility](protocol-compatibility.md) for tested GnuPG
versions and [CI coverage](continuous-integration.md) for verification limits.
