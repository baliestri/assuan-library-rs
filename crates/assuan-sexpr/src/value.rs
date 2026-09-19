use alloc::vec::Vec;
use core::fmt;

/// A canonical S-expression whose atoms borrow the input bytes.
///
/// Lists own their child storage, but atom payloads are never copied by the
/// parser. The input must outlive this value. Atoms are binary and need not be
/// UTF-8; interpret them as text explicitly when the application requires it.
///
/// `Debug` shows structure and atom lengths, not atom contents. This type does
/// not erase the caller's input. A caller parsing sensitive bytes remains
/// responsible for protecting and clearing that storage.
#[derive(Eq, PartialEq)]
pub enum Sexpr<'a> {
    /// An atom borrowing its complete binary payload, possibly empty.
    Atom(&'a [u8]),
    /// A list owning its children, possibly empty.
    List(Vec<Self>),
}

impl fmt::Debug for Sexpr<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Atom(bytes) => f.debug_struct("Atom").field("len", &bytes.len()).finish(),
            Self::List(children) => f.debug_tuple("List").field(children).finish(),
        }
    }
}
