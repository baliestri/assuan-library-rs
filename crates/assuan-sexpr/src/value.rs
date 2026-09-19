use alloc::{boxed::Box, vec::Vec};
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
      Self::Atom(bytes) => return f.debug_struct("Atom").field("len", &bytes.len()).finish(),
      Self::List(children) => return f.debug_tuple("List").field(children).finish(),
    }
  }
}

/// An S-expression owning both its atoms and its list structure.
///
/// Obtain one by explicitly copying a borrowed [`Sexpr`] with
/// [`Sexpr::to_owned`]. This value does not borrow the original input.
///
/// Atom storage is ordinary owned memory, without automatic secret erasure.
/// `Debug` omits atom contents.
#[derive(Eq, PartialEq)]
pub enum OwnedSexpr {
  /// An independently owned binary atom.
  Atom(Box<[u8]>),
  /// A list owning its children.
  List(Vec<Self>),
}

impl fmt::Debug for OwnedSexpr {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Atom(bytes) => return f.debug_struct("Atom").field("len", &bytes.len()).finish(),
      Self::List(children) => return f.debug_tuple("List").field(children).finish(),
    }
  }
}

impl Sexpr<'_> {
  /// Copies every atom and list into a value independent of the input.
  ///
  /// Conversion traverses the tree iteratively. Each atom receives its own
  /// allocation; list structure and traversal storage also allocate.
  /// This is an explicit copy into ordinary memory, not secret storage.
  ///
  /// # Examples
  ///
  /// ```
  /// use assuan_sexpr::{parse_complete, OwnedSexpr, ParseLimits};
  /// let owned = {
  ///     let input = Vec::from(b"3:key");
  ///     parse_complete(&input, ParseLimits::default())?.to_owned()
  /// };
  /// assert_eq!(owned, OwnedSexpr::Atom(Box::from(&b"key"[..])));
  /// # Ok::<(), assuan_sexpr::ParseError>(())
  /// ```
  #[must_use]
  pub fn to_owned(&self) -> OwnedSexpr {
    let mut parents = Vec::new();
    let mut cursor = self;
    'visit: loop {
      let mut completed = match cursor {
        Self::Atom(bytes) => OwnedSexpr::Atom(Box::from(*bytes)),
        Self::List(children) => {
          let mut remaining = children.iter();
          if let Some(first) = remaining.next() {
            parents.push((remaining, Vec::with_capacity(children.len())));
            cursor = first;
            continue 'visit;
          }
          OwnedSexpr::List(Vec::new())
        }
      };

      while let Some((mut remaining, mut owned_children)) = parents.pop() {
        owned_children.push(completed);
        if let Some(next) = remaining.next() {
          parents.push((remaining, owned_children));
          cursor = next;
          continue 'visit;
        }
        completed = OwnedSexpr::List(owned_children);
      }
      return completed;
    }
  }
}
