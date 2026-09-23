# Continuous integration

`ci.yml` checks Windows, Linux and macOS with pinned Rust 1.98.1 and current
stable toolchains. Each job generates an ignored `Cargo.lock`, reuses it
with `--locked`, and uploads the lockfile and dependency tree for diagnosis.
Consumers do not need to share a committed lockfile.

Production library checks and rustdoc run before checks with test targets or
extra features. `scripts/verify-features.ps1` checks all 16 combinations of the
facade's `client`, `server`, `macros` and `sexpr` features, then checks each pure
crate without defaults and prints its normal dependency tree.

The matrix also runs strict Clippy, unit and property tests, compile-fail UI
tests, integration tests and doctests. Three real-GnuPG fixture tests are
explicitly filtered here; `interop.yml` runs them with required GnuPG installs
on all three platforms. Literal TCP and named-pipe tests still run in the main
matrix. A passing CI matrix alone does not establish GnuPG interoperability.

Dedicated jobs run `cargo-deny`, nightly rustfmt for both workspaces, and
`actionlint`. The linter image is pinned by digest; every `.yml` and `.yaml`
workflow is enumerated in sorted order, including future release workflows.
Nightly formatting does not change the library's MSRV. The separate `fuzz.yml`
runs three bounded Linux fuzz tests, including a weekly schedule. Dependencies
are fetched with `--locked`; offline fuzz execution must leave its lock unchanged.
See [the fuzz guide](../fuzz/README.md) for targets, seeds and limitations.

## Gates for an exact commit

All three workflows preserve their normal events and expose `workflow_call`
with one required input, `source_sha`: a full commit SHA with 40 lowercase
hexadecimal characters. Branches, tags and empty values are rejected. Normal
events use `github.sha`; calls use `source_sha`.

Every checkout, including formatting and dependency auditing, uses the validated
SHA and verifies `git rev-parse HEAD`. The final `gate` job rejects failed,
cancelled or skipped dependencies and exposes the `source_sha` workflow output
only after all required jobs succeed. The release workflow requires successful
results and compares all three outputs with its prepared release commit.

Each job resolves its own ignored lockfile, then uses `--locked` for Cargo
checks. The fuzz job also checks that offline fuzz execution leaves its lockfile
unchanged. No lock distribution protocol is required between workflows.
Resolutions can differ across jobs and from the later publication job; these
checks establish behavior at the selected source commit, not identical
dependency resolutions. Diagnostic artifact names include the run ID and run
attempt, so retries do not collide with earlier diagnostics.

These gates have `contents: read`, do not declare registry secrets, and never
publish. Callers must not use `secrets: inherit`; release credentials belong only
in the publication job. Local workflow validation does not establish that remote execution or
publication has succeeded.

## Release scripts and deterministic documentation

The Linux `release_scripts` job downloads PowerShell **7.6.6** from its official
release and verifies a checked-in SHA-256 before extraction. It verifies the
running version, installs Rust 1.98.1, runs all script suites, builds rustdoc,
then generates the actual versioned site twice and compares file and ZIP hashes.
Script suites do not publish to crates.io. Workflow contract tests execute the
input, checkout and final gate scripts, including invalid SHA, failed or skipped
job and permission cases;
`actionlint` separately validates YAML, expressions and action syntax.

To reproduce from the Rust workspace with PowerShell 7.6.6:

```powershell
cargo generate-lockfile
./scripts/verify-features.ps1
./scripts/test-release.ps1
cargo doc --locked --workspace --all-features --no-deps
./scripts/verify-docs.ps1
./scripts/test-release.ps1 -Suite Workflow
```

On Windows, `./scripts/test-interop.ps1` also uses `--locked`; generate the
lock first. It requires real GnuPG tools and fails when they are unavailable.
