use crate::ParseError;

/// The maximum configurable number of nested lists.
///
/// Parsing uses an explicit stack, but the returned tree owns nested lists.
/// This ceiling also bounds their ordinary destruction and traversal depth.
pub const MAX_NESTING_DEPTH: usize = 256;

/// Validated limits on parsing work and list storage.
///
/// Defaults permit 16 MiB of input, atoms of up to 16 MiB, 65,536 nodes and
/// 64 nested lists. Atoms and lists each count as one node; atom bytes do not.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseLimits {
    input_bytes: usize,
    atom_bytes: usize,
    nodes: usize,
    depth: usize,
}

impl ParseLimits {
    /// Constructs explicit parsing limits.
    ///
    /// `max_input` includes every byte of the supplied slice, including bytes
    /// after a prefix expression. `max_atom` counts an atom's payload only.
    /// A zero atom limit permits only empty atoms; a zero depth limit permits
    /// a top-level atom but no lists.
    ///
    /// # Errors
    ///
    /// Returns [`crate::ParseErrorKind::InvalidLimits`] when `max_input` or
    /// `max_nodes` is zero, `max_atom` exceeds `max_input`, or `max_depth`
    /// exceeds [`MAX_NESTING_DEPTH`]. No allocation is performed.
    pub const fn new(
        max_input: usize,
        max_atom: usize,
        max_nodes: usize,
        max_depth: usize,
    ) -> Result<Self, ParseError> {
        if max_input == 0 || max_nodes == 0 || max_atom > max_input || max_depth > MAX_NESTING_DEPTH
        {
            return Err(ParseError::invalid_limits());
        }
        Ok(Self {
            input_bytes: max_input,
            atom_bytes: max_atom,
            nodes: max_nodes,
            depth: max_depth,
        })
    }

    /// Returns the maximum size of the entire input slice, in bytes.
    #[must_use]
    pub const fn max_input(self) -> usize {
        self.input_bytes
    }

    /// Returns the maximum number of payload bytes in an atom.
    #[must_use]
    pub const fn max_atom(self) -> usize {
        self.atom_bytes
    }

    /// Returns the maximum combined number of atoms and lists.
    #[must_use]
    pub const fn max_nodes(self) -> usize {
        self.nodes
    }

    /// Returns the maximum number of lists containing an expression.
    ///
    /// An empty top-level list has depth one; a top-level atom has depth zero.
    #[must_use]
    pub const fn max_depth(self) -> usize {
        self.depth
    }
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            input_bytes: 16 * 1024 * 1024,
            atom_bytes: 16 * 1024 * 1024,
            nodes: 65_536,
            depth: 64,
        }
    }
}
