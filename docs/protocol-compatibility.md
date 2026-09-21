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
the independent integration tests below provide that additional coverage.

## Independent coverage

The facade integration tests use GnuPG as an independent implementation:

- `interop_agent` discovers a fresh agent with `AgentLocator`, checks `GETINFO
  version` and `NOP`, and verifies that an unknown command returns a typed remote
  error while permitting another `NOP` on the same connection.
- On Unix, `interop_server` drives our server with `gpg-connect-agent
  --raw-socket`, exercising `NOP`, `HELP`, a registered `ECHO` handler, and `BYE`.
- On Windows, a literal Tokio named-pipe peer checks server greeting, help,
  data, and termination bytes. GnuPG agent discovery uses its native loopback
  TCP socket file and nonce, not Windows named pipes.
- A literal TCP peer exercises greeting inquiries and secret binary data,
  including NUL, CR, LF, percent escaping, and a non-UTF-8 byte. These paths are
  not covered by the simple real-agent transaction. Diagnostics are redacted.

These checks establish compatibility for the listed exchanges, not every GnuPG
command, agent version, authentication mechanism, or cryptographic operation.

## Running the tests

From the Rust workspace, with GnuPG installed:

```sh
ASSUAN_GNUPG_REQUIRED=1 cargo test -p assuan-library --test interop_agent --test interop_server -- --nocapture
```

`ASSUAN_GPGCONF` and `ASSUAN_GPG_CONNECT_AGENT` select executables explicitly.
The fixture logs their versions. Without required mode, only a missing
executable allows a reported skip; installed tools that fail always fail the
test. A skip is not an interoperability pass.

On Windows, use PowerShell from the workspace:

```powershell
./scripts/test-interop.ps1
./scripts/test-interop.ps1 -Workspace # Entire workspace test suite
```

The script temporarily maps an unused drive letter to the selected GnuPG
installation and restores environment variables and the mapping in `finally`.
Both tools must come from the same `bin` directory. Portable Scoop installations
can otherwise produce socket paths too long for the agent, even with a short
homedir. This workaround changes test invocation, not installation configuration.
Do not run multiple instances of this drive-mapping script concurrently.

## Isolation and cleanup

Each fixture creates its own homedir under Cargo's target temporary directory,
with Unix mode 0700 or an inheritable current-user-only Windows DACL. Every
GnuPG subprocess receives that explicit homedir. Only that agent is launched
and killed; no user keys, passphrases, or default agent are needed.

GnuPG may place sockets outside the homedir in its own runtime or installation
directory. Discovery uses its reported path. Subprocess output is bounded and
not logged; processes and socket readiness have deadlines. Normal shutdown waits
for socket removal before deleting the homedir. A regression test also checks
the synchronous emergency drop path. Cleanup failure is reported and retains the
homedir for investigation. Forcefully terminating the test runner can bypass
destructors and PowerShell cleanup.

## Validation record

Local validation on 2026-09-21:

| Platform | GnuPG | Independent interoperability |
| --- | --- | --- |
| Linux, Docker `rust:1.98.1` | 2.4.7 | Real agent, raw Unix socket server, literal TCP passed |
| Windows, portable Scoop | 2.5.22 | Real agent via TCP/nonce, named-pipe server, literal TCP passed using short tool paths |
| macOS | Not executed locally | Pending CI execution |

`.github/workflows/interop.yml` installs GnuPG explicitly on isolated Linux,
macOS, and Windows runners and requires all applicable interoperability tests.
The presence of a job does not mean that platform has been validated; hosted
workflow execution remains pending until pushed and run.
