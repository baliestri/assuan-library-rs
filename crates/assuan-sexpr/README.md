# assuan-sexpr

A parser for canonical S-expressions with borrowed binary atoms, bounded
resource use and no I/O. It supports `no_std` with `alloc` by disabling default
features.

```rust
use assuan_sexpr::{parse_complete, ParseLimits, Sexpr};

let value = parse_complete(b"(3:foo0:)", ParseLimits::default())?;
assert_eq!(value, Sexpr::List(vec![Sexpr::Atom(b"foo"), Sexpr::Atom(b"")]));
# Ok::<(), assuan_sexpr::ParseError>(())
```

Atoms borrow the original input; lists allocate their child storage. Atom data
is binary, so its declared length counts bytes, not Unicode characters. The
input must remain alive while the parsed value is used.

`parse_complete` rejects trailing data. `parse_prefix` returns the number of
bytes consumed and leaves the next expression to the caller. Neither function
ignores whitespace outside atoms or accepts non-canonical forms.

Default limits are 16 MiB of input, 16 MiB per atom, 65,536 nodes and 64 levels
of lists. `ParseLimits::new` configures these limits; supported nesting is
capped at 256 levels to bound the depth of the returned tree. The input limit
applies to the full input slice, including any unconsumed suffix.

Errors contain a category and offset, never input bytes. `Debug` on an
expression shows its structure and atom lengths without exposing contents.
The parser does not erase borrowed input; callers retain responsibility for
protecting sensitive data.

Use `Sexpr::to_owned` to copy atoms and list structure into an `OwnedSexpr` that outlives the input. `to_canonical` produces canonical bytes; `write_canonical` appends them to an existing vector. Encoding applies the default parsing limits, including to manually constructed trees. The output byte limit includes existing bytes when appending. Failures leave output contents and length unchanged. These owned values and output buffers are ordinary memory, not secret storage.

## License

MIT. See [LICENSE.md](LICENSE.md).
