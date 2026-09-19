use core::fmt;

/// The category of a canonical S-expression parsing failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ParseErrorKind {
    /// The configured limits are inconsistent or unsupported.
    InvalidLimits,
    /// The entire input slice exceeds its configured byte limit.
    InputLimit,
    /// An atom declares more bytes than permitted.
    AtomLimit,
    /// The expression contains too many atoms and lists.
    NodeLimit,
    /// A list exceeds the configured nesting limit.
    DepthLimit,
    /// An atom length cannot be represented as a `usize`.
    LengthOverflow,
    /// An atom length is not canonical, for example because of a leading zero.
    InvalidLength,
    /// A decimal atom length is not followed by a colon.
    ExpectedColon,
    /// A byte cannot begin an expression at this position.
    UnexpectedToken,
    /// The input ends before an atom or list is complete.
    UnexpectedEof,
    /// A complete expression is followed by additional bytes.
    TrailingData,
    /// Storage for the list structure could not be reserved.
    AllocationFailed,
}

/// A parsing failure with a byte offset and no copy of the input.
///
/// Configuration errors have no byte offset. All input offsets are zero-based;
/// an unexpected end of input points one byte past the last available byte.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseError {
    kind: ParseErrorKind,
    offset: Option<usize>,
}

impl ParseError {
    pub(crate) const fn at(kind: ParseErrorKind, offset: usize) -> Self {
        Self {
            kind,
            offset: Some(offset),
        }
    }

    pub(crate) const fn invalid_limits() -> Self {
        Self {
            kind: ParseErrorKind::InvalidLimits,
            offset: None,
        }
    }

    /// Returns the category without exposing any input bytes.
    #[must_use]
    pub const fn kind(self) -> ParseErrorKind {
        self.kind
    }

    /// Returns the zero-based byte offset, or `None` for invalid configuration.
    #[must_use]
    pub const fn offset(self) -> Option<usize> {
        self.offset
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let description = match self.kind {
            ParseErrorKind::InvalidLimits => "invalid S-expression limits",
            ParseErrorKind::InputLimit => "input byte limit exceeded",
            ParseErrorKind::AtomLimit => "atom byte limit exceeded",
            ParseErrorKind::NodeLimit => "expression node limit exceeded",
            ParseErrorKind::DepthLimit => "list nesting limit exceeded",
            ParseErrorKind::LengthOverflow => "atom length overflow",
            ParseErrorKind::InvalidLength => "non-canonical atom length",
            ParseErrorKind::ExpectedColon => "expected colon after atom length",
            ParseErrorKind::UnexpectedToken => "unexpected expression byte",
            ParseErrorKind::UnexpectedEof => "incomplete S-expression",
            ParseErrorKind::TrailingData => "data after complete S-expression",
            ParseErrorKind::AllocationFailed => "could not reserve expression storage",
        };
        f.write_str(description)?;
        if let Some(offset) = self.offset {
            write!(f, " at byte {offset}")?;
        }
        Ok(())
    }
}

impl core::error::Error for ParseError {}
