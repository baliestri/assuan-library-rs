# Package verification and publication

## Scope

The workspace contains seven publishable crates at version 0.1.0. No release
has been published. CI validates source, feature combinations, interoperability
and fuzz smoke tests. The manual Release workflow publishes packages and creates
release tags; see [release operations](releasing.md).
The commands below prepare and inspect local archives without registry writes.

## Prerequisites and validation

Use the pinned Rust 1.98.1 toolchain, PowerShell and `tar`. Cargo needs access
to crates.io to resolve dependencies, but this procedure requires no publishing
token. Run from the Rust workspace:

```powershell
cargo test --workspace --doc --all-features
$env:RUSTDOCFLAGS = '-D warnings -D rustdoc::broken_intra_doc_links'
cargo doc --workspace --all-features --no-deps
cargo +nightly fmt --all -- --check
```

Check every exit code. For Rust implementation changes also run affected tests
and workspace Clippy with all targets/features; review the cross-platform CI
results. Real GnuPG tests have a separate provisioning workflow. The recorded
matrix is Ubuntu/GnuPG 2.4.4 and macOS/Windows/GnuPG 2.5.22; see
[compatibility details](protocol-compatibility.md). A passing matrix does not
establish compatibility with every GnuPG version or distribution.

Review changes, obtain staging and commit approvals separately, and create
GPG-signed personal commits. Automated release commits use the bot without a
signature requirement. From a clean Git tree, run:

```powershell
cargo generate-lockfile
./scripts/verify-packages.ps1
```

The script runs `cargo package --workspace --locked --registry crates-io`. Joint
selection lets Cargo verify unpublished workspace dependencies together in its
temporary registry. It compiles the packaged crates, checks all seven archives,
audits paths and normalized dependency declarations, and reports SHA-256 hashes.
It also runs a joint cargo publish --workspace --dry-run --locked and checks
clean VCS identity. It honors Cargo's target directory from metadata. It never passes `--no-verify`
or `--allow-dirty`, publishes, reads credentials, stages files or creates commits.
If packaging fails, resolve the reported failure locally; publishing is not a
workaround. Cargo's package build is not a replacement for the test suite.

## Archive review

Each archive must contain its sources, README, MIT license, normalized manifest
and original manifest. Examples, Rust tests and compiler diagnostic fixtures are
allowed. Cargo may generate `Cargo.lock` and `.cargo_vcs_info.json`; this does not
mean the workspace's ignored lockfile should be committed.

The script rejects unexpected archive paths and local/git dependencies in the
normalized manifest. Review actual source and fixture contents for secrets:
path checks are not a secret scanner. Never include reference .NET projects,
external design/plan files, personal artifacts, build outputs or credentials.
Inspect an archive manually when investigating a failure:

```powershell
tar -tf target/package/assuan-library-0.1.0.crate
tar -xOf target/package/assuan-library-0.1.0.crate assuan-library-0.1.0/Cargo.toml
```

Internal dependencies retain a version alongside their development `path`;
Cargo removes the path when packaging. Keep versions synchronized when preparing
a release. Each crate supplies docs.rs metadata; the facade documents all public
features, while the transport's test-only binary is excluded from that selection.

## Automated release process

The [release guide](releasing.md) covers setup, manual dispatch, version preparation,
crates.io publication, versioned Pages documentation and failure recovery.
The local verification commands above perform no registry writes.
