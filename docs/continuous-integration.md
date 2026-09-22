# Continuous integration

`ci.yml` checks Windows, Linux and macOS with the pinned Rust 1.98.1 and current
stable toolchains. Each job generates its own ignored `Cargo.lock`, reuses it
with `--locked`, and uploads the lockfile and dependency tree for diagnosis.
The repository does not require consumers to share a committed lockfile.

Production library checks and rustdoc run before checks with test targets or
extra features. `scripts/verify-features.ps1` checks all 16 combinations of the
facade's `client`, `server`, `macros` and `sexpr` features, then checks each pure
crate separately without defaults and prints its normal dependency tree.

The matrix also runs strict Clippy, unit and property tests, compile-fail UI
tests, integration tests and doctests. Three real-GnuPG fixture tests are
explicitly filtered here; `interop.yml` runs them with required GnuPG installs
on all three platforms. Literal TCP and named-pipe tests still run in the main
matrix. A passing CI matrix alone does not establish GnuPG interoperability.

Dedicated jobs run `cargo-deny`, nightly rustfmt for both workspaces, and
`actionlint`. Nightly formatting is required by the repository's rustfmt options;
it does not change the library's MSRV. The separate `fuzz.yml` uses nightly on
Linux for three bounded fuzz smoke tests, including a weekly schedule. See
[the fuzz guide](../fuzz/README.md) for targets, seeds and limitations.

To reproduce the feature checks from the Rust workspace:

```powershell
cargo generate-lockfile
./scripts/verify-features.ps1
```

CI does not create releases, merge branches, tag commits, or publish crates.
Release automation remains a separate design task after package verification.
