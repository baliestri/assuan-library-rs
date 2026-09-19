# Protocol compatibility

## Agent discovery

The discovery implementation follows these versioned upstream sources:

- [GnuPG 2.4.7, tools/gpgconf.c, list_dirs](https://github.com/gpg/gnupg/blob/gnupg-2.4.7/tools/gpgconf.c).
- [libassuan 3.0.0, src/assuan-socket.c](https://github.com/gpg/libassuan/blob/libassuan-3.0.0/src/assuan-socket.c), specifically `read_port_and_nonce`, the Windows connection path, and the native socket-file writer.
- [gpgconf command reference](https://gnupg.org/documentation/manuals/gnupg26/gpgconf.1.html).

`gpgconf --list-dirs agent-socket` prints the selected pathname **without percent
escaping**, followed by a newline. The unfiltered `--list-dirs` listing uses a
different, escaped format. Discovery therefore preserves literal `%20`, spaces,
and Unix non-UTF-8 pathname bytes. It requires one absolute pathname and removes
only the final LF (or CRLF on Windows). Windows output must be UTF-8.

The executable is supplied explicitly, with an optional home directory passed
as a separate `--homedir` argument. No shell is involved. The child has a
five-second deadline and a shared one-MiB stdout/stderr budget; both pipes are
drained concurrently. Failed operations kill and wait for the child. Dropping
the discovery future uses Tokio's kill-on-drop behavior. Captured output is
excluded from diagnostics.

## Native Windows agent sockets

The native libassuan file contains a decimal TCP port, one LF, and exactly 16
binary nonce bytes. This implementation accepts ports 1 through 65535 and files
of at most 22 bytes. It rejects signs, whitespace, CRLF separators, trailing
bytes, and the Cygwin `!<socket >` format. This is deliberately stricter than
libassuan's permissive integer parsing, while accepting its native writer's
output.

Discovery reads at most 23 bytes to detect excess data, directly into protected
storage. The retained nonce has fixed protected capacity and is wiped on normal
drop. Debug output exposes neither the nonce nor the discovered pathname.
Filesystem reads run on a blocking worker; Tokio cannot forcibly cancel an OS
read already in progress, even after the discovery deadline expires.

`ResolvedEndpoint::connect` connects to IPv4 loopback and writes the complete
nonce before returning the stream. Connection and nonce transmission share one
deadline. The Assuan greeting remains unread for the session layer. Calling the
generic connector with `ResolvedEndpoint::endpoint()` alone omits this handshake;
use the resolved endpoint's own `connect` method for native agent connections.

## Validation scope

Deterministic tests use a dedicated `gpgconf_stub` executable and a TCP peer that
requires the exact binary nonce before sending a greeting. Run them with
`cargo test --workspace --features assuan-transport/test-fixtures`; the fixture
binary is disabled by default. Tests cover malformed output, process failures,
aggregate output limits, timeout cleanup, and platform-specific pathname bytes.

These fixtures exercise the documented formats without a GnuPG installation.
They do not constitute end-to-end interoperability testing with a real agent;
that validation belongs to the later interoperability stage.
