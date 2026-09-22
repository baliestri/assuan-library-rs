# Fuzz smoke tests

This private package is excluded from the seven published crates. The targets
exercise pure APIs without GnuPG, network I/O, or credentials:

- `sexpr`: bounded parsing, canonical round trips and stable re-encoding.
- `wire`: equivalent framing for every two-part split of inputs up to 64 bytes,
  input-derived splits and chunk sizes for larger inputs, parser and decoder
  robustness, sticky framing errors and partial EOF.
- `states`: client/server transitions including inquiries, cancellation and
  closure; invalid or closed sessions can never become usable again.

Install nightly and `cargo-fuzz` on Linux (Docker is suitable). From the main
workspace, copy each `fuzz/seeds/<target>/` directory into the ignored
`fuzz/corpus/<target>/` directory, then run:

```sh
cargo +nightly fuzz run sexpr -- -max_total_time=30 -max_len=4096
cargo +nightly fuzz run wire -- -max_total_time=30 -max_len=4096
cargo +nightly fuzz run states -- -max_total_time=30 -max_len=4096
```

These are short smoke tests, not exhaustive correctness or security proofs.
Only small synthetic seeds are tracked. Never seed the fuzzer with secrets.
Generated corpora, crash artifacts and lockfiles remain untracked; CI archives
crashes and its resolved lockfile for diagnosis. Turn reproducible findings into
focused regression tests in the affected crate.

Format this separate workspace with
`cargo +nightly fmt --manifest-path fuzz/Cargo.toml --all`.
